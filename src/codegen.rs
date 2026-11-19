//! cURL import and code generation.
//!
//! Everything here is pure text: the request model goes in, a snippet comes
//! out; a pasted `curl` command goes in, a request comes out.

use std::collections::HashMap;

use crate::model::{
    ApiRequest, Auth, BodyMode, FieldKind, FormField, KV, Method, expand_with,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Curl,
    Javascript,
    Python,
    Go,
    Rust,
    Httpie,
}

impl Lang {
    pub const ALL: [Lang; 6] = [
        Lang::Curl,
        Lang::Javascript,
        Lang::Python,
        Lang::Go,
        Lang::Rust,
        Lang::Httpie,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Lang::Curl => "cURL",
            Lang::Javascript => "JavaScript",
            Lang::Python => "Python",
            Lang::Go => "Go",
            Lang::Rust => "Rust",
            Lang::Httpie => "HTTPie",
        }
    }
}

struct MultipartPart {
    key: String,
    value: String,
    file: bool,
    content_type: String,
}

/// A request reduced to the things every snippet needs.
struct Prepared {
    method: Method,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
    json_body: bool,
    multipart: Vec<MultipartPart>,
    binary: Option<String>,
}

pub fn snippet(lang: Lang, req: &ApiRequest, env: &HashMap<String, String>) -> String {
    let p = prepare(req, env);
    match lang {
        Lang::Curl => curl(&p),
        Lang::Javascript => javascript(&p),
        Lang::Python => python(&p),
        Lang::Go => go(&p),
        Lang::Rust => rust(&p),
        Lang::Httpie => httpie(&p),
    }
}

pub fn curl_for(req: &ApiRequest, env: &HashMap<String, String>) -> String {
    curl(&prepare(req, env))
}

fn prepare(req: &ApiRequest, env: &HashMap<String, String>) -> Prepared {
    let url = full_url(req, env);

    let mut headers: Vec<(String, String)> = req
        .headers
        .iter()
        .filter(|kv| kv.enabled && !kv.key.trim().is_empty())
        .map(|kv| (expand_with(kv.key.trim(), env), expand_with(&kv.value, env)))
        .collect();

    match &req.auth {
        Auth::None | Auth::Inherit => {}
        Auth::OAuth2 { .. } => {
            headers.push((
                "Authorization".into(),
                "Bearer {{accessToken}}".into(),
            ));
        }
        Auth::Bearer { token } => {
            let token = expand_with(token, env);
            if !token.trim().is_empty() {
                headers.push(("Authorization".into(), format!("Bearer {}", token.trim())));
            }
        }
        Auth::Basic { user, pass } => {
            let user = expand_with(user, env);
            let pass = expand_with(pass, env);
            let encoded = base64_encode(format!("{user}:{pass}").as_bytes());
            headers.push(("Authorization".into(), format!("Basic {encoded}")));
        }
        Auth::ApiKey { header, value } => {
            let header = expand_with(header, env);
            if !header.trim().is_empty() {
                headers.push((header.trim().to_string(), expand_with(value, env)));
            }
        }
    }

    let mut body = None;
    let mut content_type = None;
    let mut multipart: Vec<MultipartPart> = Vec::new();
    let mut binary: Option<String> = None;
    match req.body_mode {
        BodyMode::None => {}
        BodyMode::Json => {
            let text = expand_with(&req.body, env);
            if !text.trim().is_empty() {
                body = Some(text);
                content_type = Some("application/json");
            }
        }
        BodyMode::Text => {
            let text = expand_with(&req.body, env);
            if !text.trim().is_empty() {
                body = Some(text);
                content_type = Some("text/plain; charset=utf-8");
            }
        }
        BodyMode::Form => {
            let fields: Vec<(String, String)> = req
                .form
                .iter()
                .filter(|kv| kv.enabled && !kv.key.trim().is_empty())
                .map(|kv| (expand_with(kv.key.trim(), env), expand_with(&kv.value, env)))
                .collect();
            if !fields.is_empty() {
                body = Some(
                    fields
                        .iter()
                        .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
                        .collect::<Vec<_>>()
                        .join("&"),
                );
                content_type = Some("application/x-www-form-urlencoded");
            }
        }
        BodyMode::Multipart => {
            headers.retain(|(key, _)| !key.eq_ignore_ascii_case("content-type"));
            for field in req
                .multipart
                .iter()
                .filter(|field| field.enabled && !field.key.trim().is_empty())
            {
                multipart.push(MultipartPart {
                    key: expand_with(field.key.trim(), env),
                    value: expand_with(&field.value, env),
                    file: field.kind == FieldKind::File,
                    content_type: expand_with(&field.content_type, env),
                });
            }
        }
        BodyMode::Binary => {
            let path = expand_with(&req.binary_path, env);
            if !path.trim().is_empty() {
                if !has_header(&headers, "content-type") {
                    headers.push((
                        "Content-Type".into(),
                        guess_content_type(&path).to_string(),
                    ));
                }
                binary = Some(path);
            }
        }
    }
    if let Some(ct) = content_type {
        if !has_header(&headers, "content-type") {
            headers.push(("Content-Type".into(), ct.to_string()));
        }
    }

    Prepared {
        method: req.method,
        url,
        headers,
        json_body: body.is_some() && req.body_mode == BodyMode::Json,
        body,
        multipart,
        binary,
    }
}

/// URL with the enabled query parameters appended, exactly like the transport
/// layer builds it, so copied snippets match what the app actually sends.
pub fn full_url(req: &ApiRequest, env: &HashMap<String, String>) -> String {
    let raw = crate::http::resolved_url(req, env);
    let pairs: Vec<(String, String)> = req
        .params
        .iter()
        .filter(|kv| kv.enabled && !kv.key.trim().is_empty())
        .map(|kv| (expand_with(kv.key.trim(), env), expand_with(&kv.value, env)))
        .collect();
    if pairs.is_empty() {
        return raw;
    }
    // Parsing would percent-encode unresolved `{{placeholders}}`, which makes
    // the copied snippet unusable; only normalise real URLs.
    if !raw.contains("{{") {
        if let Ok(mut url) = reqwest::Url::parse(&raw) {
            {
                let mut query = url.query_pairs_mut();
                for (key, value) in pairs {
                    query.append_pair(&key, &value);
                }
            }
            return url.to_string();
        }
    }
    let mut out = raw;
    let sep = if out.contains('?') { '&' } else { '?' };
    out.push(sep);
    out.push_str(
        &pairs
            .iter()
            .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
            .collect::<Vec<_>>()
            .join("&"),
    );
    out
}

// ---------------------------------------------------------------------------
// Renderers
// ---------------------------------------------------------------------------

fn curl(p: &Prepared) -> String {
    let has_payload = !p.multipart.is_empty() || p.binary.is_some() || p.body.is_some();
    let mut parts = Vec::new();
    if p.method == Method::Head {
        parts.push(format!("curl -I {}", sh_quote(&p.url)));
    } else if p.method == Method::Get && !has_payload {
        parts.push(format!("curl {}", sh_quote(&p.url)));
    } else {
        parts.push(format!(
            "curl -X {} {}",
            p.method.as_str(),
            sh_quote(&p.url)
        ));
    }
    for (key, value) in &p.headers {
        parts.push(format!("-H {}", sh_quote(&format!("{key}: {value}"))));
    }
    if !p.multipart.is_empty() {
        for part in &p.multipart {
            let mut value = if part.file {
                format!("{}=@{}", part.key, part.value)
            } else {
                format!("{}={}", part.key, part.value)
            };
            if !part.content_type.trim().is_empty() {
                value.push_str(&format!(";type={}", part.content_type));
            }
            let flag = if !part.file && part.value.starts_with('@') {
                "--form-string"
            } else {
                "-F"
            };
            parts.push(format!("{flag} {}", sh_quote(&value)));
        }
    } else if let Some(path) = &p.binary {
        parts.push(format!("--data-binary {}", sh_quote(&format!("@{path}"))));
    } else if let Some(body) = &p.body {
        parts.push(format!("--data-raw {}", sh_quote(body)));
    }
    if parts.len() == 1 {
        parts[0].clone()
    } else {
        parts.join(" \
  ")
    }
}

fn javascript(p: &Prepared) -> String {
    let needs_blob = p.binary.is_some() || p.multipart.iter().any(|part| part.file);
    let mut out = String::new();
    if needs_blob {
        out.push_str("import { openAsBlob } from \"node:fs\";\n\n");
    }
    if !p.multipart.is_empty() {
        out.push_str("const form = new FormData();\n");
        for part in &p.multipart {
            if part.file {
                out.push_str(&format!(
                    "form.append({}, await openAsBlob({}), {});\n",
                    json_string(&part.key),
                    json_string(&part.value),
                    json_string(&filename_of(&part.value))
                ));
            } else {
                out.push_str(&format!(
                    "form.append({}, {});\n",
                    json_string(&part.key),
                    json_string(&part.value)
                ));
            }
        }
        out.push('\n');
    }

    let mut opts = Vec::new();
    opts.push(format!("  method: \"{}\",", p.method.as_str()));
    let headers: Vec<&(String, String)> = p
        .headers
        .iter()
        .filter(|(key, _)| {
            !(p.multipart.len() > 0 && key.eq_ignore_ascii_case("content-type"))
        })
        .collect();
    if !headers.is_empty() {
        let rows = headers
            .iter()
            .map(|(key, value)| format!("    {}: {},", json_string(key), json_string(value)))
            .collect::<Vec<_>>()
            .join("\n");
        opts.push(format!("  headers: {{\n{rows}\n  }},"));
    }
    if !p.multipart.is_empty() {
        opts.push("  body: form,".to_string());
    } else if let Some(path) = &p.binary {
        opts.push(format!("  body: await openAsBlob({}),", json_string(path)));
    } else if let Some(body) = &p.body {
        let literal = if (looks_json(body) || body.contains('\n'))
            && !body.contains('`')
            && !body.contains("${")
        {
            format!("`{}`", body)
        } else {
            json_string(body)
        };
        opts.push(format!("  body: {literal},"));
    }
    let call = if opts.is_empty() {
        format!("const response = await fetch({});", json_string(&p.url))
    } else {
        format!(
            "const response = await fetch({}, {{\n{}\n}});",
            json_string(&p.url),
            opts.join("\n")
        )
    };
    let parse = if p.json_body || p.body.as_deref().map(looks_json).unwrap_or(false) {
        "const data = await response.json();"
    } else {
        "const data = await response.text();"
    };
    format!("{out}{call}\n\n{parse}\nconsole.log(response.status, data);")
}

fn python_value(value: &serde_json::Value, indent: usize, out: &mut String) {
    use serde_json::Value;
    let pad = " ".repeat(indent + 4);
    let close = " ".repeat(indent);
    match value {
        Value::Object(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{\n");
            for (key, val) in map {
                out.push_str(&pad);
                out.push_str(&json_string(key));
                out.push_str(": ");
                python_value(val, indent + 4, out);
                out.push_str(",\n");
            }
            out.push_str(&close);
            out.push('}');
        }
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push_str("[\n");
            for item in items {
                out.push_str(&pad);
                python_value(item, indent + 4, out);
                out.push_str(",\n");
            }
            out.push_str(&close);
            out.push(']');
        }
        Value::String(text) => out.push_str(&json_string(text)),
        Value::Bool(true) => out.push_str("True"),
        Value::Bool(false) => out.push_str("False"),
        Value::Null => out.push_str("None"),
        Value::Number(number) => out.push_str(&number.to_string()),
    }
}

fn python(p: &Prepared) -> String {
    let mut args = Vec::new();
    if !p.headers.is_empty() {
        let rows = p
            .headers
            .iter()
            .map(|(k, v)| format!("        {}: {},", json_string(k), json_string(v)))
            .collect::<Vec<_>>()
            .join("\n");
        args.push(format!("    headers={{\n{rows}\n    }},"));
    }
    if !p.multipart.is_empty() {
        let text: Vec<(&String, &String)> = p
            .multipart
            .iter()
            .filter(|part| !part.file)
            .map(|part| (&part.key, &part.value))
            .collect();
        if !text.is_empty() {
            let rows = text
                .iter()
                .map(|(k, v)| format!("        {}: {},", json_string(k), json_string(v)))
                .collect::<Vec<_>>()
                .join("\n");
            args.push(format!("    data={{\n{rows}\n    }},"));
        }
        let files: Vec<(&String, &String)> = p
            .multipart
            .iter()
            .filter(|part| part.file)
            .map(|part| (&part.key, &part.value))
            .collect();
        if !files.is_empty() {
            let rows = files
                .iter()
                .map(|(k, v)| {
                    format!(
                        "        {}: open({}, \"rb\"),",
                        json_string(k),
                        json_string(v)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            args.push(format!("    files={{\n{rows}\n    }},"));
        }
    } else if let Some(path) = &p.binary {
        args.push(format!("    data=open({}, \"rb\"),", json_string(path)));
    } else if let Some(body) = &p.body {
        let json = p
            .json_body
            .then(|| serde_json::from_str::<serde_json::Value>(body).ok())
            .flatten();
        match json {
            Some(value) => {
                let mut literal = String::new();
                python_value(&value, 4, &mut literal);
                args.push(format!("    json={literal},"));
            }
            None => args.push(format!("    data={},", json_string(body))),
        }
    }
    let func = p.method.as_str().to_lowercase();
    let call = if args.is_empty() {
        format!("response = requests.{func}({})", json_string(&p.url))
    } else {
        format!(
            "response = requests.{func}(\n    {},\n{}\n)",
            json_string(&p.url),
            args.join("\n")
        )
    };
    format!("import requests\n\n{call}\n\nprint(response.status_code)\nprint(response.text)")
}

fn go(p: &Prepared) -> String {
    if !p.multipart.is_empty() {
        return go_multipart(p);
    }
    if let Some(path) = &p.binary {
        return go_binary(p, path);
    }
    let mut imports = vec!["\"fmt\"", "\"io\"", "\"net/http\""];
    let body_expr = match &p.body {
        Some(body) => {
            imports.push("\"strings\"");
            format!("strings.NewReader({})", go_string(body))
        }
        None => "nil".to_string(),
    };
    let mut out = String::new();
    out.push_str("package main\n\nimport (\n");
    for import in &imports {
        out.push_str(&format!("    {import}\n"));
    }
    out.push_str(")\n\nfunc main() {\n");
    out.push_str(&format!(
        "    req, err := http.NewRequest({}, {}, {})\n",
        go_string(p.method.as_str()),
        go_string(&p.url),
        body_expr
    ));
    out.push_str("    if err != nil {\n        panic(err)\n    }\n");
    for (key, value) in &p.headers {
        out.push_str(&format!(
            "    req.Header.Set({}, {})\n",
            go_string(key),
            go_string(value)
        ));
    }
    go_finish(&mut out);
    out
}

fn go_multipart(p: &Prepared) -> String {
    let has_files = p.multipart.iter().any(|part| part.file);
    let mut imports = vec![
        "\"bytes\"",
        "\"fmt\"",
        "\"io\"",
        "\"mime/multipart\"",
        "\"net/http\"",
    ];
    if has_files {
        imports.push("\"os\"");
    }
    let mut out = String::new();
    out.push_str("package main\n\nimport (\n");
    for import in &imports {
        out.push_str(&format!("    {import}\n"));
    }
    out.push_str(")\n\nfunc main() {\n");
    out.push_str("    var body bytes.Buffer\n");
    out.push_str("    writer := multipart.NewWriter(&body)\n");
    for part in &p.multipart {
        if part.file {
            out.push_str("    {\n");
            out.push_str(&format!(
                "        part, err := writer.CreateFormFile({}, {})\n",
                go_string(&part.key),
                go_string(&filename_of(&part.value))
            ));
            out.push_str("        if err != nil {\n            panic(err)\n        }\n");
            out.push_str(&format!(
                "        file, err := os.Open({})\n",
                go_string(&part.value)
            ));
            out.push_str("        if err != nil {\n            panic(err)\n        }\n");
            out.push_str("        _, _ = io.Copy(part, file)\n");
            out.push_str("        _ = file.Close()\n");
            out.push_str("    }\n");
        } else {
            out.push_str(&format!(
                "    _ = writer.WriteField({}, {})\n",
                go_string(&part.key),
                go_string(&part.value)
            ));
        }
    }
    out.push_str("    _ = writer.Close()\n\n");
    out.push_str(&format!(
        "    req, err := http.NewRequest({}, {}, &body)\n",
        go_string(p.method.as_str()),
        go_string(&p.url)
    ));
    out.push_str("    if err != nil {\n        panic(err)\n    }\n");
    out.push_str("    req.Header.Set(\"Content-Type\", writer.FormDataContentType())\n");
    for (key, value) in &p.headers {
        if key.eq_ignore_ascii_case("content-type") {
            continue;
        }
        out.push_str(&format!(
            "    req.Header.Set({}, {})\n",
            go_string(key),
            go_string(value)
        ));
    }
    go_finish(&mut out);
    out
}

fn go_binary(p: &Prepared, path: &str) -> String {
    let imports = vec!["\"fmt\"", "\"io\"", "\"net/http\"", "\"os\""];
    let mut out = String::new();
    out.push_str("package main\n\nimport (\n");
    for import in &imports {
        out.push_str(&format!("    {import}\n"));
    }
    out.push_str(")\n\nfunc main() {\n");
    out.push_str(&format!(
        "    file, err := os.Open({})\n",
        go_string(path)
    ));
    out.push_str("    if err != nil {\n        panic(err)\n    }\n");
    out.push_str("    defer file.Close()\n\n");
    out.push_str(&format!(
        "    req, err := http.NewRequest({}, {}, file)\n",
        go_string(p.method.as_str()),
        go_string(&p.url)
    ));
    out.push_str("    if err != nil {\n        panic(err)\n    }\n");
    for (key, value) in &p.headers {
        out.push_str(&format!(
            "    req.Header.Set({}, {})\n",
            go_string(key),
            go_string(value)
        ));
    }
    go_finish(&mut out);
    out
}

fn go_finish(out: &mut String) {
    out.push_str("\n    res, err := http.DefaultClient.Do(req)\n");
    out.push_str("    if err != nil {\n        panic(err)\n    }\n");
    out.push_str("    defer res.Body.Close()\n\n");
    out.push_str("    body, _ := io.ReadAll(res.Body)\n");
    out.push_str("    fmt.Println(res.Status)\n");
    out.push_str("    fmt.Println(string(body))\n}");
}

fn rust(p: &Prepared) -> String {
    if !p.multipart.is_empty() {
        return rust_multipart(p);
    }
    let body_expr = match &p.binary {
        Some(path) => format!("std::fs::read({})?", rust_string(path)),
        None => match &p.body {
            Some(body) => rust_string(body),
            None => "String::new()".to_string(),
        },
    };
    let method = match p.method {
        Method::Get => "get",
        Method::Post => "post",
        Method::Put => "put",
        Method::Patch => "patch",
        Method::Delete => "delete",
        Method::Head => "head",
        Method::Options => "request(reqwest::Method::OPTIONS, ",
    };
    let open = if p.method == Method::Options {
        format!(".{method}{})", rust_string(&p.url))
    } else {
        format!(".{method}({})", rust_string(&p.url))
    };
    let mut builder = format!("    let response = client\n        {open}\n");
    for (key, value) in &p.headers {
        builder.push_str(&format!(
            "        .header({}, {})\n",
            rust_string(key),
            rust_string(value)
        ));
    }
    if p.binary.is_some() || p.body.is_some() {
        builder.push_str(&format!("        .body({body_expr})\n"));
    }
    builder.push_str("        .send()?;\n");
    format!(
        "fn main() -> Result<(), Box<dyn std::error::Error>> {{\n    let client = reqwest::blocking::Client::new();\n{builder}\n    println!(\"{{}}\", response.status());\n    println!(\"{{}}\", response.text()?);\n    Ok(())\n}}\n"
    )
}

fn rust_multipart(p: &Prepared) -> String {
    let mut form = String::from("    let form = reqwest::blocking::multipart::Form::new()");
    for part in &p.multipart {
        if part.file {
            form.push_str(&format!(
                "\n        .file({}, {})?",
                rust_string(&part.key),
                rust_string(&part.value)
            ));
        } else {
            form.push_str(&format!(
                "\n        .text({}, {})",
                rust_string(&part.key),
                rust_string(&part.value)
            ));
        }
    }
    form.push_str(";\n");
    let mut builder = String::new();
    for (key, value) in &p.headers {
        if key.eq_ignore_ascii_case("content-type") {
            continue;
        }
        builder.push_str(&format!(
            "        .header({}, {})\n",
            rust_string(key),
            rust_string(value)
        ));
    }
    builder.push_str("        .multipart(form)\n        .send()?;\n");
    format!(
        "// Requires reqwest with the \"multipart\" feature.\nfn main() -> Result<(), Box<dyn std::error::Error>> {{\n    let client = reqwest::blocking::Client::new();\n{form}    let response = client\n        .{}({})\n{builder}\n    println!(\"{{}}\", response.status());\n    println!(\"{{}}\", response.text()?);\n    Ok(())\n}}\n",
        match p.method {
            Method::Get => "get",
            Method::Post => "post",
            Method::Put => "put",
            Method::Patch => "patch",
            Method::Delete => "delete",
            Method::Head => "head",
            Method::Options => "request",
        },
        rust_string(&p.url)
    )
}

fn httpie(p: &Prepared) -> String {
    if !p.multipart.is_empty() {
        let mut parts = vec![format!(
            "http --form {} {}",
            p.method.as_str(),
            sh_quote(&p.url)
        )];
        for part in &p.multipart {
            let item = if part.file {
                format!("{}@{}", part.key, part.value)
            } else {
                format!("{}={}", part.key, part.value)
            };
            parts.push(sh_quote(&item));
        }
        return parts.join(" \\\n  ");
    }
    let mut parts = vec![format!("http {} {}", p.method.as_str(), sh_quote(&p.url))];
    for (key, value) in &p.headers {
        parts.push(sh_quote(&format!("{key}: {value}")));
    }
    if let Some(path) = &p.binary {
        parts.push(format!("< {}", sh_quote(path)));
    } else if let Some(body) = &p.body {
        parts.push(format!("--raw {}", sh_quote(body)));
    }
    parts.join(" \\\n  ")
}

fn filename_of(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file")
        .to_string()
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

// ---------------------------------------------------------------------------
// cURL import
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct ImportedRequest {
    pub method: Method,
    pub url: String,
    pub name: String,
    pub params: Vec<KV>,
    pub headers: Vec<KV>,
    pub auth: Auth,
    pub body_mode: BodyMode,
    pub body: String,
    pub form: Vec<KV>,
    pub multipart: Vec<FormField>,
    pub binary_path: String,
}

impl ImportedRequest {
    pub fn into_request(self) -> ApiRequest {
        let mut req = ApiRequest::new(&self.name, self.method, &self.url);
        req.params = self.params;
        req.headers = self.headers;
        req.auth = self.auth;
        req.body_mode = self.body_mode;
        req.body = self.body;
        req.form = self.form;
        req.multipart = self.multipart;
        req.binary_path = self.binary_path;
        req
    }
}

pub fn parse_curl(input: &str) -> Result<ImportedRequest, String> {
    let tokens = tokenize(input)?;
    let start = tokens
        .iter()
        .position(|t| *t == "curl" || t.ends_with("/curl"))
        .ok_or_else(|| "That does not look like a cURL command.".to_string())?;

    let mut url: Option<String> = None;
    let mut method: Option<Method> = None;
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut data: Option<String> = None;
    let mut multipart: Vec<FormField> = Vec::new();
    let mut binary_path: Option<String> = None;
    let mut basic: Option<(String, String)> = None;
    let mut as_get = false;
    let mut head = false;

    let mut i = start + 1;
    while i < tokens.len() {
        let tok = tokens[i].clone();
        i += 1;

        if let Some(long) = tok.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((name, value)) => (format!("--{name}"), Some(value.to_string())),
                None => (format!("--{long}"), None),
            };
            let value = |i: &mut usize| -> Result<String, String> {
                match &inline {
                    Some(v) => Ok(v.clone()),
                    None => {
                        let v = tokens
                            .get(*i)
                            .cloned()
                            .ok_or_else(|| format!("Missing value for {name}."))?;
                        *i += 1;
                        Ok(v)
                    }
                }
            };
            match name.as_str() {
                "--request" => method = Some(parse_method(&value(&mut i)?)?),
                "--url" => url = Some(value(&mut i)?),
                "--header" => headers.push(parse_header(&value(&mut i)?)),
                "--data" | "--data-raw" | "--data-ascii" | "--data-urlencode" => {
                    let v = value(&mut i)?;
                    if name != "--data-raw" {
                        if let Some(path) = v.strip_prefix('@') {
                            binary_path = Some(path.to_string());
                            continue;
                        }
                    }
                    data = Some(match data {
                        Some(prev) => format!("{prev}&{v}"),
                        None => v,
                    });
                }
                "--data-binary" => {
                    let v = value(&mut i)?;
                    if let Some(path) = v.strip_prefix('@') {
                        binary_path = Some(path.to_string());
                    } else {
                        data = Some(match data {
                            Some(prev) => format!("{prev}&{v}"),
                            None => v,
                        });
                    }
                }
                "--json" => {
                    let v = value(&mut i)?;
                    data = Some(match data {
                        Some(prev) => format!("{prev}&{v}"),
                        None => v,
                    });
                    headers.push(("Content-Type".into(), "application/json".into()));
                    headers.push(("Accept".into(), "application/json".into()));
                }
                "--user" => basic = Some(parse_basic(&value(&mut i)?)),
                "--cookie" => headers.push(("Cookie".into(), value(&mut i)?)),
                "--user-agent" => headers.push(("User-Agent".into(), value(&mut i)?)),
                "--referer" => headers.push(("Referer".into(), value(&mut i)?)),
                "--head" => head = true,
                "--get" => as_get = true,
                "--form" | "--form-string" => {
                    let raw = value(&mut i)?;
                    multipart.push(parse_form_field(&raw, name == "--form-string"));
                }
                "--upload-file" => {
                    return Err("File uploads (-T) cannot be imported yet.".into());
                }
                // Options we understand but do not model.
                "--max-time" | "--connect-timeout" | "--retry" | "--retry-delay" | "--output"
                | "--proxy" | "--cacert" | "--cert" | "--key" | "--resolve" | "--interface"
                | "--limit-rate" | "--range" | "--write-out" | "--cookie-jar" => {
                    let _ = value(&mut i);
                }
                // Flag-only options that do not change the request we build.
                "--location" | "--location-trusted" | "--silent" | "--show-error" | "--insecure"
                | "--compressed" | "--verbose" | "--include" | "--fail" | "--fail-with-body"
                | "--no-buffer" | "--globoff" | "--http1.1" | "--http2" | "--http2-prior-knowledge"
                | "--tlsv1.2" | "--tlsv1.3" | "--ipv4" | "--ipv6" | "--no-progress-meter" => {}
                _ => {}
            }
            continue;
        }

        if tok.starts_with('-') && tok.len() > 1 {
            let rest = &tok[1..];
            let mut chars = rest.char_indices();
            while let Some((idx, flag)) = chars.next() {
                let attached = &rest[idx + flag.len_utf8()..];
                let take = |i: &mut usize| -> Result<String, String> {
                    if !attached.is_empty() {
                        Ok(attached.to_string())
                    } else {
                        let v = tokens
                            .get(*i)
                            .cloned()
                            .ok_or_else(|| format!("Missing value for -{flag}."))?;
                        *i += 1;
                        Ok(v)
                    }
                };
                match flag {
                    'X' => {
                        method = Some(parse_method(&take(&mut i)?)?);
                        break;
                    }
                    'H' => {
                        headers.push(parse_header(&take(&mut i)?));
                        break;
                    }
                    'd' => {
                        let v = take(&mut i)?;
                        data = Some(match data {
                            Some(prev) => format!("{prev}&{v}"),
                            None => v,
                        });
                        break;
                    }
                    'u' => {
                        basic = Some(parse_basic(&take(&mut i)?));
                        break;
                    }
                    'b' => {
                        headers.push(("Cookie".into(), take(&mut i)?));
                        break;
                    }
                    'A' => {
                        headers.push(("User-Agent".into(), take(&mut i)?));
                        break;
                    }
                    'e' => {
                        headers.push(("Referer".into(), take(&mut i)?));
                        break;
                    }
                    'F' => {
                        let raw = take(&mut i)?;
                        multipart.push(parse_form_field(&raw, false));
                        break;
                    }
                    'T' => return Err("File uploads (-T) cannot be imported yet.".into()),
                    'I' => head = true,
                    'G' => as_get = true,
                    'o' | 'm' | 'x' | 'w' | 'r' | 'c' => {
                        let _ = take(&mut i)?;
                        break;
                    }
                    _ => {}
                }
            }
            continue;
        }

        if url.is_some() {
            return Err("The command contains more than one URL.".into());
        }
        url = Some(tok);
    }

    let url = url.ok_or_else(|| "No URL found in the cURL command.".to_string())?;

    let method = method.unwrap_or_else(|| {
        if head {
            Method::Head
        } else if as_get {
            Method::Get
        } else if data.is_some() || !multipart.is_empty() || binary_path.is_some() {
            Method::Post
        } else {
            Method::Get
        }
    });

    let (path, query, fragment) = split_url(&url);
    let mut params = parse_query(&query);
    let mut body = data.unwrap_or_default();

    if as_get {
        params.extend(parse_query(body.trim()));
        body.clear();
    }

    let content_type = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.to_ascii_lowercase())
        .unwrap_or_default();

    let mut form = Vec::new();
    let mut body_mode = BodyMode::None;
    if !multipart.is_empty() {
        body_mode = BodyMode::Multipart;
        body.clear();
    } else if binary_path.is_some() {
        body_mode = BodyMode::Binary;
        body.clear();
    } else if !body.trim().is_empty() {
        if content_type.contains("x-www-form-urlencoded") {
            form = parse_query(body.trim());
            body.clear();
            body_mode = BodyMode::Form;
        } else if content_type.contains("json") || looks_json(&body) {
            body_mode = BodyMode::Json;
        } else {
            body_mode = BodyMode::Text;
        }
    }

    // Authorization headers become structured auth so the UI can show them.
    let mut auth = Auth::None;
    let mut kept_headers = Vec::new();
    for (key, value) in headers {
        if key.eq_ignore_ascii_case("authorization") {
            let (scheme, rest) = match value.split_once(' ') {
                Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
                None => (String::new(), ""),
            };
            if scheme == "bearer" && !rest.trim().is_empty() {
                auth = Auth::Bearer {
                    token: rest.trim().to_string(),
                };
                continue;
            }
            if scheme == "basic" {
                let credentials = base64_decode(rest.trim())
                    .and_then(|bytes| String::from_utf8(bytes).ok())
                    .and_then(|text| {
                        let (user, pass) = text.split_once(':')?;
                        Some((user.to_string(), pass.to_string()))
                    });
                if let Some((user, pass)) = credentials {
                    auth = Auth::Basic { user, pass };
                    continue;
                }
            }
        }
        if key.eq_ignore_ascii_case("x-api-key") {
            auth = Auth::ApiKey {
                header: key.clone(),
                value: value.clone(),
            };
            continue;
        }
        kept_headers.push(KV::pair(&key, &value));
    }
    if let Some((user, pass)) = basic {
        auth = Auth::Basic { user, pass };
    }

    let name = name_from_url(&path).unwrap_or_else(|| "Imported request".to_string());

    Ok(ImportedRequest {
        method,
        url: format!("{path}{fragment}"),
        name,
        params,
        headers: kept_headers,
        auth,
        body_mode,
        body,
        form,
        multipart,
        binary_path: binary_path.unwrap_or_default(),
    })
}

fn parse_form_field(raw: &str, force_text: bool) -> FormField {
    let (key, rest) = raw.split_once('=').unwrap_or((raw, ""));
    let mut kind = FieldKind::Text;
    let mut value = rest.to_string();
    let mut content_type = String::new();
    if !force_text {
        if let Some(stripped) = rest.strip_prefix('@').or_else(|| rest.strip_prefix('<')) {
            kind = FieldKind::File;
            value = stripped.to_string();
        }
    }
    if let Some(index) = value.find(";type=") {
        content_type = value[index + 6..].to_string();
        value.truncate(index);
    }
    FormField {
        enabled: true,
        key: key.trim().to_string(),
        value,
        kind,
        content_type: content_type.trim().to_string(),
    }
}

fn parse_header(raw: &str) -> (String, String) {
    let raw = raw.trim();
    if let Some((key, value)) = raw.split_once(':') {
        (key.trim().to_string(), value.trim().to_string())
    } else {
        let key = raw.trim_end_matches(';').trim();
        (key.to_string(), String::new())
    }
}

fn parse_basic(raw: &str) -> (String, String) {
    match raw.split_once(':') {
        Some((user, pass)) => (user.to_string(), pass.to_string()),
        None => (raw.to_string(), String::new()),
    }
}

fn parse_method(raw: &str) -> Result<Method, String> {
    let upper = raw.trim().to_ascii_uppercase();
    Method::ALL
        .iter()
        .copied()
        .find(|m| m.as_str() == upper)
        .ok_or_else(|| format!("Unknown HTTP method \"{raw}\"."))
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

fn tokenize(input: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_token = false;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.peek().copied() {
                Some('\n') => {
                    chars.next();
                }
                Some('\r') => {
                    chars.next();
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                }
                Some(next) => {
                    chars.next();
                    cur.push(next);
                    in_token = true;
                }
                None => {}
            },
            '^' => {
                let mut look = chars.clone();
                if matches!(look.next(), Some('\r') | Some('\n')) {
                    chars.next();
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                } else {
                    cur.push('^');
                    in_token = true;
                }
            }
            '\'' => {
                in_token = true;
                for ch in chars.by_ref() {
                    if ch == '\'' {
                        break;
                    }
                    cur.push(ch);
                }
            }
            '"' => {
                in_token = true;
                while let Some(ch) = chars.next() {
                    match ch {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => cur.push('\n'),
                            Some('t') => cur.push('\t'),
                            Some('r') => cur.push('\r'),
                            Some(other) => cur.push(other),
                            None => {}
                        },
                        other => cur.push(other),
                    }
                }
            }
            '$' if chars.peek() == Some(&'\'') => {
                chars.next();
                in_token = true;
                while let Some(ch) = chars.next() {
                    match ch {
                        '\'' => break,
                        '\\' => push_ansi_escape(&mut cur, &mut chars)?,
                        other => cur.push(other),
                    }
                }
            }
            c if c.is_whitespace() => {
                if in_token {
                    out.push(std::mem::take(&mut cur));
                    in_token = false;
                }
            }
            c => {
                cur.push(c);
                in_token = true;
            }
        }
    }
    if in_token {
        out.push(cur);
    }
    Ok(out)
}

fn push_ansi_escape(
    out: &mut String,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<(), String> {
    match chars.next() {
        Some('n') => out.push('\n'),
        Some('t') => out.push('\t'),
        Some('r') => out.push('\r'),
        Some('0') => out.push('\0'),
        Some('\\') => out.push('\\'),
        Some('\'') => out.push('\''),
        Some('"') => out.push('"'),
        Some('x') => {
            let mut hex = String::new();
            for _ in 0..2 {
                if let Some(c) = chars.next() {
                    hex.push(c);
                }
            }
            if let Ok(value) = u8::from_str_radix(&hex, 16) {
                out.push(value as char);
            }
        }
        Some('u') => {
            let mut hex = String::new();
            for _ in 0..4 {
                if let Some(c) = chars.next() {
                    hex.push(c);
                }
            }
            if let Ok(value) = u32::from_str_radix(&hex, 16) {
                if let Some(c) = char::from_u32(value) {
                    out.push(c);
                }
            }
        }
        Some(other) => out.push(other),
        None => {}
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn has_header(headers: &[(String, String)], name: &str) -> bool {
    headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(name))
}

fn looks_json(text: &str) -> bool {
    let trimmed = text.trim_start();
    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return false;
    }
    serde_json::from_str::<serde_json::Value>(text).is_ok()
}

fn split_url(url: &str) -> (String, String, String) {
    let url = url.trim();
    let (rest, fragment) = match url.split_once('#') {
        Some((rest, frag)) => (rest, format!("#{frag}")),
        None => (url, String::new()),
    };
    let (path, query) = match rest.split_once('?') {
        Some((path, query)) => (path.to_string(), query.to_string()),
        None => (rest.to_string(), String::new()),
    };
    (path, query, fragment)
}

fn parse_query(query: &str) -> Vec<KV> {
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .filter_map(|part| {
            let (key, value) = match part.split_once('=') {
                Some((key, value)) => (key, value),
                None => (part, ""),
            };
            let key = percent_decode(key);
            if key.trim().is_empty() {
                return None;
            }
            Some(KV::pair(&key, &percent_decode(value)))
        })
        .collect()
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(value) => {
                        out.push(value);
                        i += 3;
                    }
                    None => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn name_from_url(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches('/');
    let mut segments: Vec<&str> = trimmed
        .split('/')
        .filter(|s| !s.is_empty() && !s.contains(':'))
        .collect();
    let mut last = segments.pop()?;
    let short_id = last.len() == 36 && last.contains('-');
    if last.chars().all(|c| c.is_ascii_digit()) || short_id {
        if let Some(previous) = segments.pop() {
            if !previous.contains('.') {
                last = previous;
            }
        }
    }
    if last.contains('.') && last.split('.').last().map(|e| e.len() <= 4).unwrap_or(false) {
        if let Some(previous) = segments.pop() {
            last = previous;
        }
    }
    let words = last.replace(['-', '_'], " ");
    let mut chars = words.chars();
    let first = chars.next()?;
    let mut name = first.to_uppercase().to_string();
    name.push_str(chars.as_str());
    let name: String = name.chars().take(48).collect();
    (!name.trim().is_empty()).then_some(name)
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn go_string(value: &str) -> String {
    json_string(value)
}

fn rust_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn urlencode(value: &str) -> String {
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

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(input: &[u8]) -> String {
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf: u32 = 0;
    let mut bits = 0;
    for ch in input.bytes() {
        if ch == b'=' {
            break;
        }
        let value = match ch {
            b'A'..=b'Z' => ch - b'A',
            b'a'..=b'z' => ch - b'a' + 26,
            b'0'..=b'9' => ch - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        } as u32;
        buf = (buf << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xFF) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_a_post_with_headers_and_body() {
        let parsed = parse_curl(
            "curl -X POST 'https://api.example.com/v1/users?page=2&per_page=20' \\\n  -H 'Content-Type: application/json' \\\n  -H 'Authorization: Bearer abc123' \\\n  -d '{\"name\":\"Ada\",\"admin\":true}'",
        )
        .unwrap();
        assert_eq!(parsed.method, Method::Post);
        assert_eq!(parsed.url, "https://api.example.com/v1/users");
        assert_eq!(parsed.params.len(), 2);
        assert_eq!(parsed.params[0].key, "page");
        assert_eq!(parsed.params[1].value, "20");
        assert!(matches!(parsed.auth, Auth::Bearer { .. }));
        assert_eq!(parsed.body_mode, BodyMode::Json);
        assert_eq!(parsed.name, "Users");
        assert!(parsed
            .headers
            .iter()
            .any(|h| h.key == "Content-Type" && h.value == "application/json"));
    }

    #[test]
    fn imports_basic_auth_and_double_quotes() {
        let parsed = parse_curl(
            r#"curl "https://example.com/token" -u "user:pa ss" -H "Accept: application/json""#,
        )
        .unwrap();
        match parsed.auth {
            Auth::Basic { user, pass } => {
                assert_eq!(user, "user");
                assert_eq!(pass, "pa ss");
            }
            other => panic!("expected basic auth, got {other:?}"),
        }
        assert_eq!(parsed.method, Method::Get);
    }

    #[test]
    fn get_style_data_becomes_query_parameters() {
        let parsed =
            parse_curl("curl -G https://example.com/search -d 'q=rust lang' -d 'page=1'").unwrap();
        assert_eq!(parsed.method, Method::Get);
        assert_eq!(parsed.body_mode, BodyMode::None);
        assert_eq!(parsed.params.len(), 2);
        assert_eq!(parsed.params[0].key, "q");
        assert_eq!(parsed.params[0].value, "rust lang");
    }

    #[test]
    fn form_bodies_are_split_into_fields() {
        let parsed = parse_curl(
            "curl -X POST https://example.com/login -H 'Content-Type: application/x-www-form-urlencoded' --data 'user=ada&pass=lovelace'",
        )
        .unwrap();
        assert_eq!(parsed.body_mode, BodyMode::Form);
        assert_eq!(parsed.form.len(), 2);
        assert_eq!(parsed.form[1].value, "lovelace");
        assert!(parsed.body.is_empty());
    }

    #[test]
    fn json_flag_sets_content_type() {
        let parsed = parse_curl("curl --json '{\"a\":1}' https://example.com/items").unwrap();
        assert_eq!(parsed.method, Method::Post);
        assert_eq!(parsed.body_mode, BodyMode::Json);
        assert!(parsed
            .headers
            .iter()
            .any(|h| h.key == "Content-Type" && h.value == "application/json"));
    }

    #[test]
    fn ansi_c_quoting_is_understood() {
        let parsed =
            parse_curl("curl https://example.com -d $'{\"line\":\"one\\ntwo\"}'").unwrap();
        assert!(parsed.body.contains('\n'));
        assert_eq!(parsed.body, "{\"line\":\"one\ntwo\"}");
    }

    #[test]
    fn imports_multipart_fields() {
        let parsed = parse_curl(
            "curl -X POST https://example.com/upload -F 'title=Photo' -F 'file=@/tmp/a.png;type=image/png'",
        )
        .unwrap();
        assert_eq!(parsed.body_mode, BodyMode::Multipart);
        assert_eq!(parsed.multipart.len(), 2);
        assert_eq!(parsed.multipart[0].value, "Photo");
        assert_eq!(parsed.multipart[1].kind, FieldKind::File);
        assert_eq!(parsed.multipart[1].value, "/tmp/a.png");
        assert_eq!(parsed.multipart[1].content_type, "image/png");
    }

    #[test]
    fn imports_binary_bodies() {
        let parsed =
            parse_curl("curl -X POST https://example.com/upload --data-binary @/tmp/a.bin")
                .unwrap();
        assert_eq!(parsed.body_mode, BodyMode::Binary);
        assert_eq!(parsed.binary_path, "/tmp/a.bin");
    }

    #[test]
    fn snippets_render_multipart_and_binary() {
        let parsed = parse_curl(
            "curl -X POST 'https://example.com/upload' -F 'title=Photo' -F 'file=@/tmp/a.png'",
        )
        .unwrap();
        let req = parsed.into_request();
        let env = HashMap::new();
        let curl = snippet(Lang::Curl, &req, &env);
        assert!(curl.contains("-F 'title=Photo'"));
        assert!(curl.contains("-F 'file=@/tmp/a.png'"));
        assert!(snippet(Lang::Python, &req, &env).contains("files={"));
        assert!(snippet(Lang::Go, &req, &env).contains("mime/multipart"));
        assert!(snippet(Lang::Javascript, &req, &env).contains("FormData"));
        let mut binary = req.clone();
        binary.body_mode = BodyMode::Binary;
        binary.binary_path = "/tmp/a.bin".into();
        assert!(snippet(Lang::Curl, &binary, &env).contains("--data-binary '@/tmp/a.bin'"));
    }

    #[test]
    fn missing_curl_is_reported() {
        assert!(parse_curl("https://example.com").is_err());
    }

    #[test]
    fn round_trip_keeps_method_headers_and_body() {
        let parsed = parse_curl(
            "curl -X PUT 'https://api.example.com/v1/users/1' -H 'X-Trace: abc' -H 'Content-Type: application/json' -d '{\"name\":\"Ada\"}'",
        )
        .unwrap();
        let req = parsed.into_request();
        let mut env = HashMap::new();
        env.insert("baseUrl".to_string(), "ignored".to_string());
        let generated = curl_for(&req, &env);
        let again = parse_curl(&generated).unwrap();
        assert_eq!(again.method, Method::Put);
        assert_eq!(again.url, req.url);
        assert_eq!(again.body_mode, BodyMode::Json);
        assert_eq!(again.body, "{\"name\":\"Ada\"}");
        assert!(again.headers.iter().any(|h| h.key == "X-Trace"));
    }

    #[test]
    fn snippets_are_generated_for_every_language() {
        let parsed = parse_curl(
            "curl -X POST 'https://api.example.com/v1/users' -H 'Content-Type: application/json' -H 'Authorization: Bearer x' -d '{\"name\":\"Ada\"}'",
        )
        .unwrap();
        let req = parsed.into_request();
        let env = HashMap::new();
        for lang in Lang::ALL {
            let text = snippet(lang, &req, &env);
            assert!(!text.is_empty(), "{lang:?} produced nothing");
            assert!(text.contains("api.example.com"), "{lang:?} lost the URL");
        }
        assert!(snippet(Lang::Curl, &req, &env).contains("--data-raw"));
        assert!(snippet(Lang::Python, &req, &env).contains("requests.post"));
        assert!(snippet(Lang::Go, &req, &env).contains("http.NewRequest"));
        assert!(snippet(Lang::Rust, &req, &env).contains("reqwest::blocking"));
    }

    #[test]
    fn env_placeholders_are_expanded_or_kept() {
        let mut req = ApiRequest::new("Users", Method::Get, "{{baseUrl}}/users");
        req.params.push(KV::pair("page", "1"));
        let mut env = HashMap::new();
        env.insert("baseUrl".to_string(), "https://api.example.com".to_string());
        let resolved = snippet(Lang::Curl, &req, &env);
        assert!(resolved.contains("https://api.example.com/users?page=1"));
        let raw = snippet(Lang::Curl, &req, &HashMap::new());
        assert!(raw.contains("{{baseUrl}}"));
        assert!(raw.contains("page=1"));
    }

    #[test]
    fn basic_auth_encoding_matches_rfc() {
        let mut req = ApiRequest::new("x", Method::Get, "https://example.com");
        req.auth = Auth::Basic {
            user: "user".into(),
            pass: "pass".into(),
        };
        let curl = snippet(Lang::Curl, &req, &HashMap::new());
        assert!(curl.contains("Basic dXNlcjpwYXNz"));
    }
}
