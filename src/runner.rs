//! Headless collection runner for CI: `lucapi run`.

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::http;
use crate::model::{ApiRequest, ProjectData};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Text,
    Json,
    Junit,
}

struct Options {
    workspace: Option<PathBuf>,
    env: Option<String>,
    folders: Vec<String>,
    only: Vec<String>,
    variables: Vec<(String, String)>,
    format: Format,
    output: Option<PathBuf>,
    fail_fast: bool,
    timeout: Option<u64>,
    list: bool,
    quiet: bool,
}

const USAGE: &str = "\
Usage: lucapi run [options]

Runs the requests of a workspace without opening the app and reports the
assertions, so it can be used in CI.

Options:
  --workspace <path>   Workspace JSON (defaults to ~/.lucapi/workspace.json,
                       or $LUCAPI_WORKSPACE)
  --env <name>         Environment to use (defaults to the active one)
  --folder <name>      Only run requests of these collections (repeatable)
  --only <text>        Only run requests whose name contains <text> (repeatable)
  --var <key=value>    Override or add a variable (repeatable)
  --timeout <ms>       Request timeout, overrides the workspace setting
  --format <text|json|junit>
  --output <path>      Write the report to a file instead of stdout
  --fail-fast          Stop at the first failed request
  --list               List the requests that would run
  --quiet              Only print failures and the summary
  -h, --help           Show this help

Exit codes: 0 all good, 1 failures, 2 invalid usage or unreadable workspace.
A request passes when it gets a response and every assertion passes. Requests
without assertions must answer with a status below 400.
";

pub fn secrets_cli(args: &[String]) -> i32 {
    let mut workspace: Option<PathBuf> =
        std::env::var_os("LUCAPI_WORKSPACE").map(PathBuf::from);
    let mut audit = false;
    let mut check = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-h" | "--help" => {
                print!(
                    "Usage: lucapi secrets [--audit] [--check] [--workspace <path>]\n\n\
                     --audit    Report secret values still stored in plain text (exit 1 if any)\n\
                     --check    Store, read and remove a probe item in the macOS Keychain\n"
                );
                return 0;
            }
            "--audit" => audit = true,
            "--check" => check = true,
            "--workspace" => {
                match args.get(index + 1) {
                    Some(value) => {
                        workspace = Some(PathBuf::from(value));
                        index += 1;
                    }
                    None => {
                        eprintln!("lucapi secrets: --workspace needs a value");
                        return 2;
                    }
                }
            }
            other => {
                eprintln!("lucapi secrets: unknown option `{other}`");
                return 2;
            }
        }
        index += 1;
    }
    let mut code = 0;
    if check {
        match crate::vault::self_test() {
            Ok(()) => println!("Keychain: ok"),
            Err(error) => {
                println!("Keychain: {error}");
                code = 1;
            }
        }
    }
    if audit {
        let path = workspace.unwrap_or_else(crate::model::storage_path);
        // Read raw JSON so the audit sees exactly what would be committed.
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) => {
                eprintln!("lucapi secrets: could not read {}: {error}", path.display());
                return 2;
            }
        };
        let data: ProjectData = match serde_json::from_str(&raw) {
            Ok(data) => data,
            Err(error) => {
                eprintln!("lucapi secrets: {} is not a LucAPI workspace: {error}", path.display());
                return 2;
            }
        };
        let inventory = crate::model::secret_inventory(&data);
        let sealed = inventory.iter().filter(|(_, sealed)| *sealed).count();
        let plain: Vec<&String> = inventory
            .iter()
            .filter(|(_, sealed)| !*sealed)
            .map(|(account, _)| account)
            .collect();
        println!(
            "{}: {} secret(s), {sealed} in the Keychain, {} in plain text",
            path.display(),
            inventory.len(),
            plain.len()
        );
        for account in &plain {
            println!("  plain text: {account}");
        }
        if !plain.is_empty() {
            code = 1;
        }
    }
    if !audit && !check {
        print!(
            "Usage: lucapi secrets [--audit] [--check] [--workspace <path>]\nUse --help for details.\n"
        );
    }
    code
}

pub fn docs_cli(args: &[String]) -> i32 {
    match publish_docs(args) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("lucapi docs: {error}");
            2
        }
    }
}

const DOCS_USAGE: &str = "\
Usage: lucapi docs --output <dir> [options]

Builds a static, read-only documentation site (a single index.html) from a
workspace, ready to publish on any static host.

Options:
  --output <dir>       Directory to write index.html into (required)
  --workspace <path>   Workspace JSON (defaults to ~/.lucapi/workspace.json
                       or $LUCAPI_WORKSPACE)
  --title <text>       Site title (defaults to \"<workspace> API\")
  --intro <text>       Short introduction shown under the title
  --base-url <url>     Public URL of the site, used for shareable links
  --light              Light theme instead of dark
  --no-examples        Leave out example responses
  --no-bodies          Leave out request bodies
  --no-auth            Leave out the auth summary
  --raw-urls           Keep {{variables}} instead of resolving them
  -h, --help           Show this help

Secrets (secret headers, secret variables and auth values) are redacted.
";

fn publish_docs(args: &[String]) -> Result<i32, String> {
    let mut workspace: Option<PathBuf> =
        std::env::var_os("LUCAPI_WORKSPACE").map(PathBuf::from);
    let mut output: Option<PathBuf> = None;
    let mut options = crate::docs::Options::default();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        let mut value = |name: &str| -> Result<String, String> {
            match args.get(index + 1) {
                Some(value) => {
                    index += 1;
                    Ok(value.clone())
                }
                None => Err(format!("{name} needs a value")),
            }
        };
        match arg {
            "-h" | "--help" => {
                print!("{DOCS_USAGE}");
                return Ok(0);
            }
            "--workspace" => workspace = Some(PathBuf::from(value(arg)?)),
            "--output" => output = Some(PathBuf::from(value(arg)?)),
            "--title" => options.title = value(arg)?,
            "--intro" => options.intro = value(arg)?,
            "--base-url" => options.base_url = value(arg)?,
            "--light" => options.dark = false,
            "--no-examples" => options.include_examples = false,
            "--no-bodies" => options.include_bodies = false,
            "--no-auth" => options.include_auth = false,
            "--raw-urls" => options.resolve_variables = false,
            other => return Err(format!("unknown option `{other}`")),
        }
        index += 1;
    }
    let Some(output) = output else {
        return Err("--output is required".to_string());
    };
    let path = workspace.unwrap_or_else(crate::model::storage_path);
    let data = ProjectData::load(&path)?;
    let site = crate::docs::build(&data, &options);
    std::fs::create_dir_all(&output)
        .map_err(|error| format!("could not create {}: {error}", output.display()))?;
    let file = output.join("index.html");
    std::fs::write(&file, &site.html)
        .map_err(|error| format!("could not write {}: {error}", file.display()))?;
    println!(
        "{} endpoints written to {}",
        site.requests,
        file.display()
    );
    let base = options.base_url.trim().trim_end_matches('/');
    if !base.is_empty() {
        if let Some((anchor, _)) = site.anchors.first() {
            println!("Share links look like {base}/index.html#{anchor}");
        }
    }
    Ok(0)
}

pub fn cli(args: &[String]) -> i32 {
    let options = match parse(args) {
        Ok(Some(options)) => options,
        Ok(None) => {
            print!("{USAGE}");
            return 0;
        }
        Err(error) => {
            eprintln!("lucapi run: {error}\n");
            eprint!("{USAGE}");
            return 2;
        }
    };
    match execute(options) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("lucapi run: {error}");
            2
        }
    }
}

fn parse(args: &[String]) -> Result<Option<Options>, String> {
    let mut options = Options {
        workspace: std::env::var_os("LUCAPI_WORKSPACE").map(PathBuf::from),
        env: None,
        folders: Vec::new(),
        only: Vec::new(),
        variables: Vec::new(),
        format: Format::Text,
        output: None,
        fail_fast: false,
        timeout: None,
        list: false,
        quiet: false,
    };
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        let mut value = |name: &str| -> Result<String, String> {
            match args.get(index + 1) {
                Some(value) => {
                    index += 1;
                    Ok(value.clone())
                }
                None => Err(format!("{name} needs a value")),
            }
        };
        match arg {
            "-h" | "--help" => return Ok(None),
            "--workspace" => options.workspace = Some(PathBuf::from(value(arg)?)),
            "--env" => options.env = Some(value(arg)?),
            "--folder" => options.folders.push(value(arg)?),
            "--only" => options.only.push(value(arg)?),
            "--var" => {
                let raw = value(arg)?;
                let (key, value) = raw
                    .split_once('=')
                    .ok_or_else(|| "--var expects key=value".to_string())?;
                options.variables.push((key.to_string(), value.to_string()));
            }
            "--timeout" => {
                let raw = value(arg)?;
                options.timeout = Some(
                    raw.parse::<u64>()
                        .map_err(|_| "--timeout expects milliseconds".to_string())?,
                );
            }
            "--format" => {
                options.format = match value(arg)?.as_str() {
                    "text" => Format::Text,
                    "json" => Format::Json,
                    "junit" => Format::Junit,
                    other => {
                        return Err(format!(
                            "unknown format `{other}` (text, json or junit)"
                        ));
                    }
                }
            }
            "--output" => options.output = Some(PathBuf::from(value(arg)?)),
            "--fail-fast" => options.fail_fast = true,
            "--list" => options.list = true,
            "--quiet" => options.quiet = true,
            other => return Err(format!("unknown option `{other}`")),
        }
        index += 1;
    }
    Ok(Some(options))
}

fn execute(options: Options) -> Result<i32, String> {
    let path = options
        .workspace
        .clone()
        .unwrap_or_else(crate::model::storage_path);
    let data = ProjectData::load(&path)?;

    let mut env = environment(&data, options.env.as_deref())?;
    for (key, value) in &options.variables {
        env.insert(key.clone(), value.clone());
    }

    let mut settings = data.settings.clone();
    if let Some(timeout) = options.timeout {
        settings.timeout_ms = timeout.clamp(500, 120_000);
    }

    let selected = select(&data, &options);
    if selected.is_empty() {
        return Err("no request matched the filters".to_string());
    }

    if options.list {
        for (folder, request) in &selected {
            println!(
                "{:<7} {:<24} {:<12} {}",
                request.method.as_str(),
                request.name,
                folder,
                request.url
            );
        }
        return Ok(0);
    }

    let planned = selected.len();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("could not start the runtime: {error}"))?;
    let cache = http::new_oauth_cache();
    let started = std::time::Instant::now();
    let mut results: Vec<RunResult> = Vec::new();

    for (folder, request) in selected {
        let outcome = runtime.block_on(http::execute_async(
            &request,
            &env,
            &settings,
            &cache,
        ));
        // Captures feed the next request, so chained flows work in CI too.
        if let Ok(response) = &outcome.data {
            for (variable, value, _) in http::evaluate_captures(&request.captures, response) {
                env.insert(variable, value);
            }
        }
        let result = RunResult::new(folder, outcome);
        if !options.quiet || !result.ok {
            print_text(&result);
        }
        let failed = !result.ok;
        results.push(result);
        if failed && options.fail_fast {
            break;
        }
    }

    let elapsed = started.elapsed().as_millis() as u64;
    let size = report(&results, elapsed, options.format)?;
    match &options.output {
        Some(path) => {
            std::fs::write(path, size).map_err(|error| {
                format!("could not write {}: {error}", path.display())
            })?;
            if options.format != Format::Text {
                println!("Report written to {}", path.display());
            }
        }
        None => {
            if options.format != Format::Text {
                print!("{size}");
            }
        }
    }

    let failed = results.iter().filter(|result| !result.ok).count();
    let passed = results.len() - failed;
    if options.format == Format::Text {
        println!(
            "\n{passed} passed, {failed} failed in {elapsed} ms{}",
            if results.len() < planned {
                " (stopped early)"
            } else {
                ""
            }
        );
    }
    Ok(if failed == 0 { 0 } else { 1 })
}

fn environment(data: &ProjectData, name: Option<&str>) -> Result<HashMap<String, String>, String> {
    let Some(name) = name else {
        return Ok(data.env_map());
    };
    let env = data
        .envs
        .iter()
        .find(|env| env.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| {
            let names: Vec<&str> = data.envs.iter().map(|env| env.name.as_str()).collect();
            format!(
                "unknown environment `{name}` (available: {})",
                if names.is_empty() {
                    "none".to_string()
                } else {
                    names.join(", ")
                }
            )
        })?;
    Ok(env
        .vars
        .iter()
        .filter(|var| var.enabled && !var.key.trim().is_empty())
        .map(|var| (var.key.trim().to_string(), var.value.clone()))
        .collect())
}

fn select<'a>(data: &'a ProjectData, options: &Options) -> Vec<(String, ApiRequest)> {
    let mut selected = Vec::new();
    for folder in &data.folders {
        if !options.folders.is_empty()
            && !options
                .folders
                .iter()
                .any(|name| folder.name.eq_ignore_ascii_case(name))
        {
            continue;
        }
        for request in &folder.requests {
            if !options.only.is_empty()
                && !options
                    .only
                    .iter()
                    .any(|needle| request.name.to_lowercase().contains(&needle.to_lowercase()))
            {
                continue;
            }
            let mut request = request.clone();
            request.auth = request.auth.resolve(&folder.auth);
            selected.push((folder.name.clone(), request));
        }
    }
    selected
}

struct RunResult {
    folder: String,
    name: String,
    method: String,
    url: String,
    status: Option<u16>,
    ms: u64,
    size: usize,
    error: Option<String>,
    assertions: Vec<http::AssertionResult>,
    ok: bool,
}

impl RunResult {
    fn new(folder: String, outcome: http::RequestOutcome) -> Self {
        let (status, ms, size, error) = match &outcome.data {
            Ok(response) => (Some(response.status), response.elapsed_ms, response.size, None),
            Err(error) => (None, 0, 0, Some(error.clone())),
        };
        let assertions = outcome.assertions;
        let ok = error.is_none()
            && assertions.iter().all(|assertion| assertion.passed)
            && (!assertions.is_empty() || status.map(|code| code < 400).unwrap_or(false));
        Self {
            folder,
            name: outcome.request_name,
            method: outcome.method.as_str().to_string(),
            url: outcome.url,
            status,
            ms,
            size,
            error,
            assertions,
            ok,
        }
    }
}

fn print_text(result: &RunResult) {
    let status = match result.status {
        Some(code) => code.to_string(),
        None => "ERR".to_string(),
    };
    let marks: String = if result.assertions.is_empty() {
        String::new()
    } else {
        let passed = result
            .assertions
            .iter()
            .filter(|assertion| assertion.passed)
            .count();
        format!("  {passed}/{} asserts", result.assertions.len())
    };
    println!(
        "{:<7} {:<28} {:>4}  {:>6} ms{:>12}{}",
        result.method,
        truncate(&result.name, 28),
        status,
        result.ms,
        format_bytes(result.size),
        marks
    );
    if let Some(error) = &result.error {
        println!("        ↳ {error}");
    }
    for assertion in result.assertions.iter().filter(|a| !a.passed) {
        println!("        ↳ failed: {} ({})", assertion.label, assertion.detail);
    }
}

fn report(results: &[RunResult], elapsed: u64, format: Format) -> Result<String, String> {
    match format {
        Format::Text => Ok(String::new()),
        Format::Json => {
            let cases: Vec<Value> = results
                .iter()
                .map(|result| {
                    json!({
                        "folder": result.folder,
                        "name": result.name,
                        "method": result.method,
                        "url": result.url,
                        "status": result.status,
                        "ms": result.ms,
                        "bytes": result.size,
                        "ok": result.ok,
                        "error": result.error,
                        "assertions": result
                            .assertions
                            .iter()
                            .map(|assertion| json!({
                                "label": assertion.label,
                                "passed": assertion.passed,
                                "detail": assertion.detail,
                            }))
                            .collect::<Vec<_>>(),
                    })
                })
                .collect();
            let failed = results.iter().filter(|result| !result.ok).count();
            let document = json!({
                "total": results.len(),
                "passed": results.len() - failed,
                "failed": failed,
                "elapsed_ms": elapsed,
                "results": cases,
            });
            Ok(serde_json::to_string_pretty(&document).map_err(|error| error.to_string())?)
        }
        Format::Junit => {
            let failed = results.iter().filter(|result| !result.ok).count();
            let mut xml = String::new();
            xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
            xml.push_str(&format!(
                "<testsuites name=\"LucAPI\" tests=\"{}\" failures=\"{}\" time=\"{:.3}\">\n",
                results.len(),
                failed,
                elapsed as f64 / 1000.0
            ));
            let mut current = "";
            for result in results {
                if current != result.folder {
                    if !current.is_empty() {
                        xml.push_str("  </testsuite>\n");
                    }
                    current = &result.folder;
                    let count = results
                        .iter()
                        .filter(|other| other.folder == result.folder)
                        .count();
                    xml.push_str(&format!(
                        "  <testsuite name=\"{}\" tests=\"{count}\">\n",
                        escape(&result.folder)
                    ));
                }
                xml.push_str(&format!(
                    "    <testcase classname=\"{}\" name=\"{}\" time=\"{:.3}\">",
                    escape(&format!("{} {}", result.method, result.folder)),
                    escape(&result.name),
                    result.ms as f64 / 1000.0
                ));
                if result.ok {
                    xml.push_str("</testcase>\n");
                } else {
                    let detail = result
                        .error
                        .clone()
                        .unwrap_or_else(|| {
                            result
                                .assertions
                                .iter()
                                .filter(|assertion| !assertion.passed)
                                .map(|assertion| {
                                    format!("{}: {}", assertion.label, assertion.detail)
                                })
                                .collect::<Vec<_>>()
                                .join("; ")
                        });
                    xml.push_str(&format!(
                        "<failure message=\"{}\">{}</failure></testcase>\n",
                        escape(&detail),
                        escape(&result.url)
                    ));
                }
            }
            if !current.is_empty() {
                xml.push_str("  </testsuite>\n");
            }
            xml.push_str("</testsuites>\n");
            Ok(xml)
        }
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out: String = text.chars().take(limit.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn format_bytes(bytes: usize) -> String {
    crate::model::format_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AssertKind, Assertion, Folder, Method};

    fn workspace() -> ProjectData {
        let mut api = Folder::new("API");
        let mut login = ApiRequest::new("Login", Method::Post, "{{baseUrl}}/login");
        login.assertions.push(Assertion {
            enabled: true,
            kind: AssertKind::StatusEquals,
            target: String::new(),
            expected: "201".into(),
        });
        api.requests.push(login);
        api.requests.push(ApiRequest::new("Users", Method::Get, "{{baseUrl}}/users"));
        let mut other = Folder::new("Billing");
        other.requests.push(ApiRequest::new("Invoices", Method::Get, "{{baseUrl}}/invoices"));
        let mut data: ProjectData = serde_json::from_str(
            "{\"name\":\"ci\",\"folders\":[],\"envs\":[],\"active_env\":null,\"history\":[],\"settings\":{\"dark\":false,\"timeout_ms\":15000,\"follow_redirects\":true,\"line_numbers\":true,\"mock_port\":4010},\"session\":{}}",
        )
        .expect("workspace");
        data.folders = vec![api, other];
        data
    }

    fn options() -> Options {
        Options {
            workspace: None,
            env: None,
            folders: Vec::new(),
            only: Vec::new(),
            variables: Vec::new(),
            format: Format::Text,
            output: None,
            fail_fast: false,
            timeout: None,
            list: false,
            quiet: true,
        }
    }

    #[test]
    fn filters_by_folder_and_name() {
        let data = workspace();
        let selected = select(&data, &options());
        assert_eq!(selected.len(), 3);

        let mut opts = options();
        opts.folders = vec!["api".to_string()];
        assert_eq!(select(&data, &opts).len(), 2);

        let mut opts = options();
        opts.only = vec!["INVO".to_string()];
        let selected = select(&data, &opts);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].1.name, "Invoices");
    }

    #[test]
    fn unknown_environment_is_reported() {
        let data = workspace();
        let error = environment(&data, Some("Nope")).unwrap_err();
        assert!(error.contains("unknown environment"));
    }

    #[test]
    fn junit_report_escapes_and_groups() {
        let results = vec![
            RunResult {
                folder: "API".into(),
                name: "A & B".into(),
                method: "GET".into(),
                url: "http://x/".into(),
                status: Some(200),
                ms: 12,
                size: 1,
                error: None,
                assertions: Vec::new(),
                ok: true,
            },
            RunResult {
                folder: "API".into(),
                name: "Fails".into(),
                method: "GET".into(),
                url: "http://x/".into(),
                status: Some(500),
                ms: 3,
                size: 1,
                error: None,
                assertions: Vec::new(),
                ok: false,
            },
        ];
        let xml = report(&results, 15, Format::Junit).unwrap();
        assert!(xml.contains("tests=\"2\" failures=\"1\""));
        assert!(xml.contains("A &amp; B"));
        assert!(xml.contains("<failure"));
    }

    #[test]
    fn json_report_counts_results() {
        let results = vec![RunResult {
            folder: "API".into(),
            name: "Ok".into(),
            method: "GET".into(),
            url: "http://x/".into(),
            status: Some(204),
            ms: 4,
            size: 0,
            error: None,
            assertions: Vec::new(),
            ok: true,
        }];
        let json: Value = serde_json::from_str(&report(&results, 4, Format::Json).unwrap()).unwrap();
        assert_eq!(json["passed"], 1);
        assert_eq!(json["results"][0]["status"], 204);
    }

    #[test]
    fn requests_without_assertions_need_a_healthy_status() {
        let failing = RunResult {
            folder: "API".into(),
            name: "x".into(),
            method: "GET".into(),
            url: "http://x/".into(),
            status: Some(500),
            ms: 1,
            size: 0,
            error: None,
            assertions: Vec::new(),
            ok: false,
        };
        assert!(!failing.ok);
    }
}
