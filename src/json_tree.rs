//! Interactive JSON tree for the response pane.
//!
//! The tree is built once per body, flattened into rows only when the
//! expansion state or the search changes, and painted with uniform row heights
//! so `ScrollArea::show_rows` keeps huge payloads fast.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use eframe::egui::{
    self, Color32, FontId, Rect, Response, RichText, Sense, Stroke, TextEdit, Ui, Vec2,
    pos2, vec2,
};

use crate::theme::Palette;
use crate::widgets::{self, Icon};

const ROW_HEIGHT: f32 = 21.0;
const MAX_NODES: usize = 20_000;
const MAX_ROWS: usize = 20_000;
const INDENT: f32 = 14.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Object,
    Array,
    String,
    Number,
    Bool,
    Null,
}

#[derive(Debug)]
pub struct JsonNode {
    key: String,
    path: String,
    kind: Kind,
    /// Rendered value for scalars (strings keep their quotes).
    display: String,
    /// Raw value for the clipboard (strings without quotes).
    copy: String,
    children: Vec<JsonNode>,
}

pub struct Row {
    depth: usize,
    key: String,
    path: String,
    kind: Kind,
    display: String,
    copy: String,
    container_size: usize,
    expanded: bool,
    matched: bool,
}

pub struct TreeState {
    root: Option<JsonNode>,
    hash: u64,
    search: String,
    last_query: String,
    expanded: HashSet<String>,
    rows: Vec<Row>,
    dirty: bool,
    pub is_json: bool,
    pub matches: usize,
    truncated: bool,
}

impl Default for TreeState {
    fn default() -> Self {
        Self {
            root: None,
            hash: 0,
            search: String::new(),
            last_query: String::new(),
            expanded: HashSet::new(),
            rows: Vec::new(),
            dirty: true,
            is_json: false,
            matches: 0,
            truncated: false,
        }
    }
}

impl TreeState {
    /// Parses and caches the body when it changed. Cheap on unchanged bodies.
    pub fn prepare(&mut self, body: &str) {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        body.hash(&mut hasher);
        let hash = hasher.finish();
        if hash == self.hash && (self.root.is_some() || !self.is_json) {
            return;
        }
        self.hash = hash;
        self.search.clear();
        self.last_query.clear();
        self.root = None;
        self.is_json = false;
        self.truncated = false;
        self.rows.clear();
        self.matches = 0;
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
            let mut budget = MAX_NODES;
            let mut truncated = false;
            let root = build_node("".into(), "$".into(), &value, &mut budget, &mut truncated);
            self.truncated = truncated;
            self.is_json = true;
            self.expanded.clear();
            self.expanded.insert("$".into());
            // Small payloads read better fully expanded; big ones start folded
            // so the viewer never dumps thousands of rows on first paint.
            let mut nodes = 0;
            count_nodes(&root, &mut nodes);
            if nodes <= 200 {
                collect_containers(&root, &mut self.expanded);
            }
            self.root = Some(root);
        }
        self.dirty = true;
    }

    pub fn search_mut(&mut self) -> &mut String {
        &mut self.search
    }

    pub fn search(&self) -> &str {
        &self.search
    }

    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    fn ensure_rows(&mut self) {
        if !self.dirty && self.search == self.last_query {
            return;
        }
        self.last_query = self.search.clone();
        self.rows.clear();
        self.matches = 0;
        let Some(root) = &self.root else {
            self.dirty = false;
            return;
        };
        let query = self.search.trim().to_lowercase();
        if query.is_empty() {
            for child in &root.children {
                push_open(child, 0, &self.expanded, &mut self.rows);
            }
        } else {
            for child in &root.children {
                push_filtered(child, 0, &query, &mut self.rows, &mut self.matches);
            }
        }
        self.rows.truncate(MAX_ROWS);
        self.dirty = false;
    }

    /// Draws the tree. Returns the path the user asked to capture as a
    /// variable, if any.
    pub fn show(&mut self, ui: &mut Ui, p: &Palette) -> Option<String> {
        if !self.is_json {
            return None;
        }
        self.ensure_rows();
        let mut toggle: Option<String> = None;
        let mut capture: Option<String> = None;
        let height = ui.available_height().max(ROW_HEIGHT);
        egui::ScrollArea::vertical()
            .id_salt("json_tree_scroll")
            .auto_shrink([false, false])
            .max_height(height)
            .show_rows(ui, ROW_HEIGHT, self.rows.len(), |ui, range| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                for index in range {
                    let row = &self.rows[index];
                    let response = draw_row(ui, p, row, &mut capture);
                    if response.clicked() && row.kind_object_or_array() {
                        toggle = Some(row.path.clone());
                    }
                }
                if self.truncated {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Preview truncated: this body is very large.")
                            .size(10.5)
                            .color(p.warn),
                    );
                }
            });
        if let Some(path) = toggle {
            if self.expanded.contains(&path) {
                self.expanded.remove(&path);
            } else {
                self.expanded.insert(path);
            }
            self.dirty = true;
        }
        capture
    }
}

impl Row {
    fn kind_object_or_array(&self) -> bool {
        matches!(self.kind, Kind::Object | Kind::Array)
    }
}

fn build_node(
    key: String,
    path: String,
    value: &serde_json::Value,
    budget: &mut usize,
    truncated: &mut bool,
) -> JsonNode {
    use serde_json::Value;
    *budget = budget.saturating_sub(1);
    let (kind, display, copy) = match value {
        Value::Object(_) => (Kind::Object, String::new(), String::new()),
        Value::Array(_) => (Kind::Array, String::new(), String::new()),
        Value::String(text) => (
            Kind::String,
            serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into()),
            text.clone(),
        ),
        Value::Number(number) => (Kind::Number, number.to_string(), number.to_string()),
        Value::Bool(flag) => (Kind::Bool, flag.to_string(), flag.to_string()),
        Value::Null => (Kind::Null, "null".into(), "null".into()),
    };
    let mut children = Vec::new();
    if *budget > 0 {
        match value {
            Value::Object(map) => {
                for (child_key, child_value) in map {
                    if *budget == 0 {
                        *truncated = true;
                        break;
                    }
                    children.push(build_node(
                        child_key.clone(),
                        format!("{path}{}", path_segment(child_key)),
                        child_value,
                        budget,
                        truncated,
                    ));
                }
            }
            Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    if *budget == 0 {
                        *truncated = true;
                        break;
                    }
                    children.push(build_node(
                        format!("[{index}]"),
                        format!("{path}[{index}]"),
                        item,
                        budget,
                        truncated,
                    ));
                }
            }
            _ => {}
        }
    } else if !matches!(value, Value::Object(map) if map.is_empty())
        && !matches!(value, Value::Array(items) if items.is_empty())
        && matches!(value, Value::Object(_) | Value::Array(_))
    {
        *truncated = true;
    }
    JsonNode {
        key,
        path,
        kind,
        display,
        copy,
        children,
    }
}

fn path_segment(key: &str) -> String {
    if !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        format!(".{key}")
    } else {
        format!("[{}]", serde_json::to_string(key).unwrap_or_default())
    }
}

fn count_nodes(node: &JsonNode, total: &mut usize) {
    *total += 1;
    for child in &node.children {
        count_nodes(child, total);
    }
}

fn collect_containers(node: &JsonNode, out: &mut HashSet<String>) {
    if node.path != "$" && !node.children.is_empty() {
        out.insert(node.path.clone());
    }
    for child in &node.children {
        collect_containers(child, out);
    }
}

fn push_open(node: &JsonNode, depth: usize, expanded: &HashSet<String>, out: &mut Vec<Row>) {
    if out.len() >= MAX_ROWS {
        return;
    }
    let open = expanded.contains(&node.path);
    out.push(row_of(node, depth, open, false));
    if open {
        for child in &node.children {
            push_open(child, depth + 1, expanded, out);
        }
    }
}

fn push_filtered(node: &JsonNode, depth: usize, query: &str, out: &mut Vec<Row>, count: &mut usize) {
    if out.len() >= MAX_ROWS || !subtree_matches(node, query) {
        return;
    }
    let matched = self_matches(node, query);
    if matched {
        *count += 1;
    }
    out.push(row_of(node, depth, true, matched));
    for child in &node.children {
        push_filtered(child, depth + 1, query, out, count);
    }
}

fn row_of(node: &JsonNode, depth: usize, expanded: bool, matched: bool) -> Row {
    Row {
        depth,
        key: node.key.clone(),
        path: node.path.clone(),
        kind: node.kind,
        display: node.display.clone(),
        copy: node.copy.clone(),
        container_size: node.children.len(),
        expanded,
        matched,
    }
}

fn self_matches(node: &JsonNode, query: &str) -> bool {
    node.key.to_lowercase().contains(query) || node.copy.to_lowercase().contains(query)
}

fn subtree_matches(node: &JsonNode, query: &str) -> bool {
    self_matches(node, query) || node.children.iter().any(|child| subtree_matches(child, query))
}

fn draw_row(ui: &mut Ui, p: &Palette, row: &Row, capture: &mut Option<String>) -> Response {
    let width = ui.available_width().max(240.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT), Sense::click());
    let painter = ui.painter();
    if row.matched {
        painter.rect_filled(rect.shrink2(vec2(2.0, 1.0)), egui::CornerRadius::same(4), p.accent_soft);
    } else if response.hovered() {
        painter.rect_filled(rect.shrink2(vec2(2.0, 1.0)), egui::CornerRadius::same(4), p.hover);
    }
    let font = FontId::monospace(11.8);
    let mut x = rect.left() + 8.0 + row.depth as f32 * INDENT;
    let center_y = rect.center().y;

    if matches!(row.kind, Kind::Object | Kind::Array) {
        let chevron = Rect::from_center_size(pos2(x + 5.0, center_y), Vec2::splat(11.0));
        widgets::paint_icon(
            painter,
            chevron,
            if row.expanded {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            },
            p.faint,
            1.25,
        );
    }
    x += 15.0;

    if !row.key.is_empty() {
        let key_color = if row.key.starts_with('[') {
            p.faint
        } else {
            p.json_key
        };
        let galley = painter.layout_no_wrap(row.key.clone(), font.clone(), key_color);
        let key_width = galley.size().x;
        if x + key_width < rect.right() {
            painter.galley(pos2(x, center_y - galley.size().y * 0.5), galley, key_color);
            x += key_width;
            let colon = painter.layout_no_wrap(": ".into(), font.clone(), p.faint);
            if x + colon.size().x < rect.right() {
                let colon_width = colon.size().x;
                painter.galley(
                    pos2(x, center_y - colon.size().y * 0.5),
                    colon,
                    p.faint,
                );
                x += colon_width;
            }
        } else {
            x = rect.right();
        }
    }

    if x < rect.right() {
        let (text, color) = match row.kind {
            Kind::Object => (
                if row.expanded {
                    "{".to_string()
                } else {
                    format!("{{ {} keys }}", row.container_size)
                },
                p.muted,
            ),
            Kind::Array => (
                if row.expanded {
                    "[".to_string()
                } else {
                    format!("[ {} items ]", row.container_size)
                },
                p.muted,
            ),
            Kind::String => (row.display.clone(), p.json_string),
            Kind::Number => (row.display.clone(), p.json_number),
            Kind::Bool | Kind::Null => (row.display.clone(), p.json_literal),
        };
        let max = (rect.right() - x - 6.0).max(0.0);
        let mut galley = painter.layout_no_wrap(text.clone(), font, color);
        if galley.size().x > max {
            galley = painter.layout_no_wrap(elide(painter, &text, max), FontId::monospace(11.8), color);
        }
        painter.galley(pos2(x, center_y - galley.size().y * 0.5), galley, color);
    }
    let hint = if !row.copy.is_empty() && row.copy.chars().count() > 90 {
        row.copy.clone()
    } else {
        row.path.clone()
    };
    let _ = response.context_menu(|ui| {
        if !row.copy.is_empty() && ui.button("Copy value").clicked() {
            ui.ctx().copy_text(row.copy.clone());
            ui.close_menu();
        }
        if ui.button("Copy path").clicked() {
            ui.ctx().copy_text(row.path.clone());
            ui.close_menu();
        }
        ui.separator();
        if ui.button("Capture as variable…").clicked() {
            *capture = Some(row.path.clone());
            ui.close_menu();
        }
    });
    response.on_hover_text(hint)
}

fn elide(painter: &egui::Painter, text: &str, max_width: f32) -> String {
    let font = FontId::monospace(11.8);
    let full = painter.layout_no_wrap(text.to_string(), font.clone(), Color32::WHITE);
    if full.size().x <= max_width {
        return text.to_string();
    }
    let mut out = String::new();
    for ch in text.chars() {
        let candidate = format!("{out}{ch}…");
        let galley = painter.layout_no_wrap(candidate, font.clone(), Color32::WHITE);
        if galley.size().x > max_width {
            break;
        }
        out.push(ch);
    }
    format!("{out}…")
}

/// Compact search box used by the response toolbar.
pub fn search_box(ui: &mut Ui, p: &Palette, state: &mut TreeState) -> Response {
    let width = 178.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, 24.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(6), p.panel_alt);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(6),
        Stroke::new(1.0_f32, p.border),
        egui::StrokeKind::Inside,
    );
    let icon_rect = Rect::from_center_size(
        pos2(rect.left() + 12.0, rect.center().y),
        Vec2::splat(12.0),
    );
    widgets::paint_icon(painter, icon_rect, Icon::Search, p.faint, 1.25);
    let edit_rect = Rect::from_min_max(
        pos2(rect.left() + 24.0, rect.top() + 2.0),
        pos2(rect.right() - 6.0, rect.bottom() - 2.0),
    );
    ui.put(
        edit_rect,
        TextEdit::singleline(state.search_mut())
            .frame(false)
            .hint_text(RichText::new("Search JSON").size(11.0).color(p.faint))
            .font(FontId::monospace(11.0))
            .margin(egui::Margin::symmetric(0, 2)),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(body: &str) -> TreeState {
        let mut state = TreeState::default();
        state.prepare(body);
        state
    }

    #[test]
    fn ignores_non_json_bodies() {
        let state = tree("<html>nope</html>");
        assert!(!state.is_json);
    }

    #[test]
    fn builds_expected_paths_and_expands_small_payloads() {
        let mut state = tree(r#"{"data":[{"email":"a@b.c"}],"page":1}"#);
        state.ensure_rows();
        let paths: Vec<&str> = state.rows.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["$.data", "$.data[0]", "$.data[0].email", "$.page"]
        );
    }

    #[test]
    fn search_keeps_ancestors_and_counts_matches() {
        let mut state = tree(r#"{"data":[{"email":"ada@example.com"},{"email":"grace@example.com"}],"total":2}"#);
        state.search = "grace".into();
        state.ensure_rows();
        let paths: Vec<&str> = state.rows.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(paths, vec!["$.data", "$.data[1]", "$.data[1].email"]);
        assert_eq!(state.matches, 1);
    }

    #[test]
    fn collapsing_a_container_hides_its_children() {
        let mut state = tree(r#"{"a":{"b":1}}"#);
        state.expanded.clear();
        state.expanded.insert("$".into());
        state.dirty = true;
        state.ensure_rows();
        let paths: Vec<&str> = state.rows.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(paths, vec!["$.a"]);
    }

    #[test]
    fn big_payloads_start_collapsed() {
        let body = format!("{{\"items\":[{}]}}", "1,".repeat(400).trim_end_matches(','));
        let mut state = tree(&body);
        state.ensure_rows();
        let paths: Vec<&str> = state.rows.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(paths, vec!["$.items"]);
    }

    #[test]
    fn deeply_nested_documents_are_capped() {
        let body = format!("[{}]", "1,".repeat(MAX_NODES + 10).trim_end_matches(','));
        let state = tree(&body);
        assert!(state.truncated);
    }
}
