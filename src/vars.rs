//! `{{variable}}` scanning, highlighting and autocomplete helpers.

use std::collections::HashSet;

use eframe::egui::{Color32, FontId, TextFormat, text::LayoutJob};

use crate::model::{ApiRequest, Auth, BodyMode};

/// Byte ranges of `{{...}}` occurrences together with the variable name.
pub fn spans(text: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some(start) = text[cursor..].find("{{") {
        let start = cursor + start;
        let Some(end) = text[start + 2..].find("}}") else {
            break;
        };
        let end = start + 2 + end + 2;
        let name = text[start + 2..end - 2].trim().to_string();
        if !name.is_empty() {
            out.push((start, end, name));
        }
        cursor = end;
    }
    out
}

pub fn undefined(text: &str, defined: &HashSet<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (_, _, name) in spans(text) {
        if !defined.contains(&name) && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// Undefined variables across every editable field of a request.
pub fn request_undefined(request: &ApiRequest, defined: &HashSet<String>) -> Vec<String> {
    let mut texts: Vec<String> = Vec::new();
    let mut push = |text: &str| {
        if !text.trim().is_empty() {
            texts.push(text.to_string());
        }
    };
    push(&request.url);
    for kv in &request.params {
        push(&kv.key);
        push(&kv.value);
    }
    for kv in &request.headers {
        push(&kv.key);
        push(&kv.value);
    }
    match request.body_mode {
        BodyMode::Json | BodyMode::Text => push(&request.body),
        BodyMode::Form => {
            for kv in &request.form {
                push(&kv.key);
                push(&kv.value);
            }
        }
        BodyMode::Multipart => {
            for field in &request.multipart {
                push(&field.key);
                push(&field.value);
                push(&field.content_type);
            }
        }
        BodyMode::Binary => push(&request.binary_path),
        BodyMode::None => {}
    }
    match &request.auth {
        Auth::Bearer { token } => push(token),
        Auth::Basic { user, pass } => {
            push(user);
            push(pass);
        }
        Auth::ApiKey { header, value } => {
            push(header);
            push(value);
        }
        Auth::OAuth2 {
            token_url,
            client_id,
            client_secret,
            scope,
            authorization_url,
            ..
        } => {
            push(token_url);
            push(client_id);
            push(client_secret);
            push(scope);
            push(authorization_url);
        }
        Auth::Inherit | Auth::None => {}
    }
    let mut out: Vec<String> = Vec::new();
    for text in texts {
        for name in undefined(&text, defined) {
            if !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}

pub fn layout_job(
    text: &str,
    defined: &HashSet<String>,
    font: FontId,
    text_color: Color32,
    accent: Color32,
    danger: Color32,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let mut last = 0;
    for (start, end, name) in spans(text) {
        if start > last {
            append(&mut job, &text[last..start], &font, text_color);
        }
        let color = if defined.contains(&name) {
            accent
        } else {
            danger
        };
        append(&mut job, &text[start..end], &font, color);
        last = end;
    }
    if last < text.len() {
        append(&mut job, &text[last..], &font, text_color);
    }
    job
}

fn append(job: &mut LayoutJob, text: &str, font: &FontId, color: Color32) {
    job.append(
        text,
        0.0,
        TextFormat {
            font_id: font.clone(),
            color,
            ..Default::default()
        },
    );
}

/// If the text ends with an unterminated `{{prefix`, returns its start and
/// prefix so the caller can offer variable completions.
pub fn open_suggestion(text: &str) -> Option<(usize, String)> {
    let start = text.rfind("{{")?;
    let rest = &text[start + 2..];
    if rest.contains("}}") || rest.contains('{') || rest.contains('}') {
        return None;
    }
    if rest.chars().any(|ch| ch.is_whitespace()) {
        return None;
    }
    Some((start, rest.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defined(names: &[&str]) -> HashSet<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn finds_spans_and_names() {
        let text = "{{baseUrl}}/users?token={{ token }}&x={{missing}}";
        let spans = spans(text);
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].2, "baseUrl");
        assert_eq!(spans[1].2, "token");
        assert_eq!(spans[2].2, "missing");
        assert_eq!(&text[spans[0].0..spans[0].1], "{{baseUrl}}");
    }

    #[test]
    fn reports_undefined_variables_once() {
        let vars = defined(&["baseUrl", "token"]);
        let missing = undefined("{{baseUrl}}/{{token}}/{{missing}}/{{missing}}", &vars);
        assert_eq!(missing, vec!["missing".to_string()]);
    }

    #[test]
    fn layout_marks_defined_and_missing_differently() {
        let vars = defined(&["token"]);
        let job = layout_job(
            "Bearer {{token}} {{unknown}}",
            &vars,
            FontId::monospace(12.0),
            Color32::WHITE,
            Color32::BLUE,
            Color32::RED,
        );
        assert_eq!(job.sections.len(), 4);
        assert_eq!(job.sections[1].format.color, Color32::BLUE);
        assert_eq!(job.sections[3].format.color, Color32::RED);
    }

    #[test]
    fn suggests_only_after_an_unterminated_opening() {
        assert_eq!(open_suggestion("{{base"), Some((0, "base".to_string())));
        assert_eq!(open_suggestion("{{}}"), None);
        assert_eq!(open_suggestion("{{a b"), None);
        assert_eq!(open_suggestion("hello"), None);
        assert_eq!(open_suggestion("{{a}} {{b"), Some((6, "b".to_string())));
    }

    #[test]
    fn checks_a_whole_request() {
        let mut request = ApiRequest::new("t", crate::model::Method::Get, "{{baseUrl}}/users");
        request.headers.push(crate::model::KV::pair("Authorization", "Bearer {{token}}"));
        request.body = "{\"id\":\"{{userId}}\"}".into();
        request.body_mode = BodyMode::Json;
        let missing = request_undefined(&request, &defined(&["baseUrl"]));
        assert_eq!(missing, vec!["token".to_string(), "userId".to_string()]);
    }
}
