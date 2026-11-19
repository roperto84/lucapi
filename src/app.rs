//! LucAPI — an API design and testing workspace.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Frame, Layout, Margin,
    RichText, ScrollArea, Sense, Stroke, TextEdit, Ui, Vec2, pos2, vec2,
};

use crate::codegen;
use crate::docs;
use crate::history::History;
use crate::http::{self, AssertionResult, MockRoute, MockServer, RequestOutcome, ResponseData};
use crate::exporter::{self, ExportDoc, Format as ExportFormat};
use crate::importer::{self, ImportPlan};
use crate::json_tree::{self, TreeState};
use crate::oauth;
use crate::model::{
    ApiRequest, AssertKind, Assertion, Auth, BodyMode, Capture, CaptureSource, FieldKind,
    Folder, FormField, HistoryEntry, Id, KV, Method, OAuthGrant, ProjectData, expand_with,
    format_bytes, format_ms, now_secs, relative_time,
};
use crate::theme::{self, Palette};
use crate::widgets::{self, Btn, Icon};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Nav {
    Apis,
    History,
    Environments,
    Tests,
    Mock,
    Docs,
    Settings,
}

impl Nav {
    fn label(self) -> &'static str {
        match self {
            Nav::Apis => "APIs",
            Nav::History => "History",
            Nav::Environments => "Environments",
            Nav::Tests => "Tests",
            Nav::Mock => "Mock server",
            Nav::Docs => "Docs",
            Nav::Settings => "Settings",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Nav::Apis => Icon::Api,
            Nav::History => Icon::History,
            Nav::Environments => Icon::Globe,
            Nav::Tests => Icon::Flask,
            Nav::Mock => Icon::Server,
            Nav::Docs => Icon::Book,
            Nav::Settings => Icon::Gear,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReqTab {
    Params,
    Body,
    Headers,
    Auth,
    Assertions,
    Capture,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RespTab {
    Body,
    Headers,
    Cookies,
    Tests,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RespView {
    Tree,
    Pretty,
    Raw,
}

#[derive(Clone)]
struct StoredResponse {
    data: Result<ResponseData, String>,
    assertions: Vec<AssertionResult>,
    at: u64,
}

struct Toast {
    text: String,
    at: Instant,
    tone: ToastTone,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ToastTone {
    Neutral,
    Success,
    Danger,
}

struct RenameState {
    target: RenameTarget,
    text: String,
}

enum RenameTarget {
    Project,
    Folder(Id),
    Request(Id),
}

enum SideAction {
    Open(Id),
    ToggleFolder(Id),
    NewRequest(Option<Id>),
    NewFolder,
    RenameFolder(Id),
    RenameRequest(Id),
    DeleteFolder(Id),
    DeleteRequest(Id),
    RenameProject,
    CopyCurl(Id),
    OpenSnippets(Id),
    ImportCurl(Option<Id>),
    FolderAuth(Id),
}

struct SnippetsState {
    request_id: Id,
    lang: codegen::Lang,
    resolved: bool,
}

struct ImportState {
    text: String,
    folder: Option<Id>,
    error: Option<String>,
}

struct CaptureDialog {
    request_id: Id,
    path: String,
    variable: String,
}

struct PublishState {
    title: String,
    intro: String,
    base_url: String,
    include_examples: bool,
    include_bodies: bool,
    include_auth: bool,
    dark: bool,
    resolve_variables: bool,
    published: Option<String>,
}

struct FolderAuthState {
    folder_id: Id,
    name: String,
    auth: Auth,
}

struct ExportState {
    format: ExportFormat,
    doc: ExportDoc,
    saved: Option<String>,
}

#[derive(Default)]
struct ApiImportState {
    text: String,
    source: String,
    plan: Option<ImportPlan>,
    error: Option<String>,
}

pub struct ShotState {
    pub path: PathBuf,
    pub frames: u32,
    pub requested: bool,
}

pub struct LaunchOptions {
    pub screenshot: Option<PathBuf>,
    pub view: Option<String>,
    pub dark: bool,
    pub snippets: bool,
    pub import: bool,
    /// Used by the screenshot harness to exercise import end to end.
    pub import_now: Option<String>,
    /// Screenshot-harness helpers: open a request by name and one of its tabs.
    pub request: Option<String>,
    pub tab: Option<String>,
    /// Screenshot-harness helpers: preload or run an OpenAPI/Postman import.
    pub api_import: Option<String>,
    pub api_import_run: Option<String>,
    /// Screenshot-harness helper: open the export dialog ("openapi"/"postman").
    pub export: Option<String>,
    /// A URL passed on the command line (`open -a LucAPI https://…`).
    pub url: Option<String>,
}

pub struct LucApiApp {
    data: ProjectData,
    history: History<ProjectData>,
    oauth: http::OAuthCache,
    nav: Nav,
    open_tabs: Vec<Id>,
    active_tab: Option<Id>,
    responses: HashMap<Id, Arc<StoredResponse>>,
    runtime: tokio::runtime::Runtime,
    task: Option<tokio::task::JoinHandle<RequestOutcome>>,
    sending: Option<Id>,
    test_task: Option<tokio::task::JoinHandle<(Vec<RequestOutcome>, u64)>>,
    test_results: Vec<RequestOutcome>,
    test_duration_ms: u64,
    test_running: bool,
    mock: MockServer,
    mock_dirty: bool,
    search: String,
    req_tab: ReqTab,
    resp_tab: RespTab,
    resp_view: RespView,
    json_tree: TreeState,
    response_height: f32,
    env_selected: Option<Id>,
    revealed: HashSet<(Id, usize)>,
    toast: Option<Toast>,
    dirty: bool,
    last_edit: Instant,
    rename: Option<RenameState>,
    snippets: Option<SnippetsState>,
    import: Option<ImportState>,
    capture_dialog: Option<CaptureDialog>,
    api_import: Option<ApiImportState>,
    export: Option<ExportState>,
    folder_auth: Option<FolderAuthState>,
    publish: Option<PublishState>,
    oauth_pending: Option<Receiver<Result<(), String>>>,
    pending_copy: Option<String>,
    confirm_reset: bool,
    shot: Option<ShotState>,
    last_saved: Option<Instant>,
    settings_flash: Option<(Instant, String)>,
    _native_menu: Option<crate::menu::NativeMenu>,
}

impl LucApiApp {
    pub fn new(cc: &eframe::CreationContext<'_>, launch: LaunchOptions) -> Self {
        let mut data = crate::model::load_or_seed();
        if launch.dark {
            data.settings.dark = true;
        }
        let shot = launch.screenshot.map(|path| ShotState {
            path,
            frames: 0,
            requested: false,
        });
        let nav = match launch.view.as_deref() {
            Some("history") => Nav::History,
            Some("environments") => Nav::Environments,
            Some("tests") => Nav::Tests,
            Some("mock") => Nav::Mock,
            Some("docs") => Nav::Docs,
            Some("settings") => Nav::Settings,
            _ => Nav::Apis,
        };
        let p = Palette::of(data.settings.dark);
        theme::install_fonts(&cc.egui_ctx);
        theme::apply_style(&cc.egui_ctx, &p);

        let mut open_tabs = match &data.session.open_tabs {
            Some(tabs) => tabs.clone(),
            None => {
                let mut fallback = Vec::new();
                if let Some(req) = data.folders.first().and_then(|f| f.requests.first()) {
                    fallback.push(req.id);
                }
                if let Some(req) = data.folders.get(1).and_then(|f| f.requests.first()) {
                    fallback.push(req.id);
                }
                fallback
            }
        };
        open_tabs.retain(|id| data.find_request(*id).is_some());
        open_tabs.dedup();
        let active_tab = data
            .session
            .active_tab
            .filter(|id| open_tabs.contains(id))
            .or_else(|| open_tabs.first().copied());
        let env_selected = data.active_env;
        let history_seed = data.clone();
        let native_menu = crate::menu::install();

        let mut app = Self {
            data,
            history: History::new(history_seed),
            oauth: http::new_oauth_cache(),
            nav,
            open_tabs,
            active_tab,
            responses: HashMap::new(),
            runtime: tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime"),
            task: None,
            sending: None,
            test_task: None,
            test_results: Vec::new(),
            test_duration_ms: 0,
            test_running: false,
            mock: MockServer::default(),
            mock_dirty: false,
            search: String::new(),
            req_tab: ReqTab::Params,
            resp_tab: RespTab::Body,
            resp_view: RespView::Tree,
            json_tree: TreeState::default(),
            response_height: 318.0,
            env_selected,
            revealed: HashSet::new(),
            toast: None,
            dirty: false,
            last_edit: Instant::now(),
            rename: None,
            snippets: None,
            import: None,
            capture_dialog: None,
            api_import: None,
            export: None,
            folder_auth: None,
            publish: None,
            oauth_pending: None,
            pending_copy: None,
            confirm_reset: false,
            shot,
            last_saved: None,
            settings_flash: None,
            _native_menu: native_menu,
        };

        if launch.snippets {
            if let Some(id) = app.active_tab {
                app.snippets = Some(SnippetsState {
                    request_id: id,
                    lang: codegen::Lang::Curl,
                    resolved: true,
                });
            }
        }
        if launch.import {
            app.import = Some(ImportState {
                text: String::new(),
                folder: app.data.folders.first().map(|f| f.id),
                error: None,
            });
        }
        if let Some(text) = launch.import_now {
            let _ = app.add_imported_request(&text, None);
        }
        if let Some(path) = &launch.api_import {
            if let Ok(text) = std::fs::read_to_string(path) {
                let mut state = ApiImportState {
                    source: path
                        .rsplit('/')
                        .next()
                        .unwrap_or_default()
                        .to_string(),
                    text,
                    ..Default::default()
                };
                match importer::detect_and_parse(&state.text) {
                    Ok(plan) => state.plan = Some(plan),
                    Err(error) => state.error = Some(error),
                }
                app.api_import = Some(state);
            }
        }
        if let Some(format) = &launch.export {
            let format = if format == "postman" {
                ExportFormat::Postman
            } else {
                ExportFormat::OpenApi
            };
            app.export = Some(ExportState {
                format,
                doc: exporter::export(&app.data, format),
                saved: None,
            });
        }
        if let Some(path) = &launch.api_import_run {
            if let Ok(text) = std::fs::read_to_string(path) {
                if let Ok(plan) = importer::detect_and_parse(&text) {
                    app.apply_import_plan(plan);
                }
            }
        }
        if let Some(name) = &launch.request {
            let needle = name.to_lowercase();
            if let Some(id) = app
                .data
                .all_requests()
                .iter()
                .find(|req| req.name.to_lowercase().contains(&needle))
                .map(|req| req.id)
            {
                app.open_request(id);
            }
        }
        if let Some(tab) = &launch.tab {
            app.req_tab = match tab.as_str() {
                "body" => ReqTab::Body,
                "headers" => ReqTab::Headers,
                "auth" => ReqTab::Auth,
                "tests" => ReqTab::Assertions,
                "capture" => ReqTab::Capture,
                _ => ReqTab::Params,
            };
        }
        if let Some(url) = &launch.url {
            app.ingest_url(url);
        }
        app.sync_session();
        app.history.reset(app.data.clone());
        app
    }

    fn palette(&self) -> Palette {
        Palette::of(self.data.settings.dark)
    }

    fn defined_vars(&self) -> std::collections::HashSet<String> {
        self.data.env_map().keys().cloned().collect()
    }

    fn touch(&mut self) {
        self.dirty = true;
        self.history.mark_dirty();
        self.last_edit = Instant::now();
    }

    /// Marks the workspace for saving without creating an undo step (UI state,
    /// automatic captures, history entries).
    fn touch_ui(&mut self) {
        self.dirty = true;
        self.last_edit = Instant::now();
    }

    fn sync_session(&mut self) {
        self.data.session.open_tabs = Some(self.open_tabs.clone());
        self.data.session.active_tab = self.active_tab;
    }

    fn session_changed(&mut self) {
        self.sync_session();
        self.dirty = true;
        self.last_edit = Instant::now();
    }

    fn undo(&mut self) {
        match self.history.undo(&self.data) {
            Some(previous) => {
                self.restore_snapshot(previous);
                self.toast(ToastTone::Neutral, "Undid the last change");
            }
            None => self.toast(ToastTone::Neutral, "Nothing to undo"),
        }
    }

    fn redo(&mut self) {
        match self.history.redo() {
            Some(next) => {
                self.restore_snapshot(next);
                self.toast(ToastTone::Neutral, "Redid the change");
            }
            None => self.toast(ToastTone::Neutral, "Nothing to redo"),
        }
    }

    fn restore_snapshot(&mut self, data: ProjectData) {
        self.data = data;
        self.open_tabs.retain(|id| self.data.find_request(*id).is_some());
        if let Some(active) = self.active_tab {
            if !self.open_tabs.contains(&active) {
                self.active_tab = self.open_tabs.first().copied();
            }
        }
        self.env_selected = self.data.active_env;
        self.responses.clear();
        self.mock_dirty = true;
        self.sync_session();
        self.dirty = true;
        self.last_edit = Instant::now();
        self.save_now();
    }

    fn save_now(&mut self) {
        match crate::model::save_with_report(&self.data) {
            Ok(report) => {
                self.dirty = false;
                self.last_saved = Some(Instant::now());
                if !report.failed.is_empty() {
                    self.toast(
                        ToastTone::Danger,
                        format!(
                            "Saved, but {} secret(s) could not reach the Keychain and stayed in the workspace",
                            report.failed.len()
                        ),
                    );
                }
            }
            Err(err) => self.toast(ToastTone::Danger, format!("Could not save: {err}")),
        }
    }

    fn toast(&mut self, tone: ToastTone, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: text.into(),
            at: Instant::now(),
            tone,
        });
    }

    fn apply_side_action(&mut self, action: SideAction) {
        match action {
            SideAction::Open(id) => self.open_request(id),
            SideAction::ToggleFolder(id) => {
                if let Some(folder) = self.data.folders.iter_mut().find(|f| f.id == id) {
                    folder.open = !folder.open;
                }
                self.touch_ui();
            }
            SideAction::NewRequest(folder_id) => {
                let target = folder_id
                    .or_else(|| self.data.folders.first().map(|f| f.id))
                    .unwrap_or_else(|| {
                        let folder = Folder::new("Untitled collection");
                        let id = folder.id;
                        self.data.folders.push(folder);
                        id
                    });
                if let Some(folder) = self.data.folders.iter_mut().find(|f| f.id == target) {
                    let req = ApiRequest::new("Untitled request", Method::Get, "");
                    let id = req.id;
                    folder.open = true;
                    folder.requests.push(req);
                    self.touch();
                    self.open_request(id);
                    self.req_tab = ReqTab::Params;
                    self.toast(ToastTone::Neutral, "New request added");
                }
            }
            SideAction::NewFolder => {
                self.data.folders.push(Folder::new("New collection"));
                self.touch();
                self.toast(ToastTone::Neutral, "Collection created");
            }
            SideAction::RenameFolder(id) => {
                if let Some(folder) = self.data.folders.iter().find(|f| f.id == id) {
                    self.rename = Some(RenameState {
                        target: RenameTarget::Folder(id),
                        text: folder.name.clone(),
                    });
                }
            }
            SideAction::RenameRequest(id) => {
                if let Some(req) = self.data.find_request(id) {
                    self.rename = Some(RenameState {
                        target: RenameTarget::Request(id),
                        text: req.name.clone(),
                    });
                }
            }
            SideAction::DeleteFolder(id) => {
                let removed: Vec<Id> = self
                    .data
                    .folders
                    .iter()
                    .find(|f| f.id == id)
                    .map(|f| f.requests.iter().map(|r| r.id).collect())
                    .unwrap_or_default();
                self.data.remove_folder(id);
                self.open_tabs.retain(|t| !removed.contains(t));
                for rid in removed {
                    self.responses.remove(&rid);
                }
                if let Some(active) = self.active_tab {
                    if !self.open_tabs.contains(&active) {
                        self.active_tab = self.open_tabs.first().copied();
                    }
                }
                self.touch();
                self.session_changed();
                self.toast(ToastTone::Neutral, "Collection deleted");
            }
            SideAction::DeleteRequest(id) => {
                self.data.remove_request(id);
                self.open_tabs.retain(|t| *t != id);
                self.responses.remove(&id);
                if self.active_tab == Some(id) {
                    self.active_tab = self.open_tabs.first().copied();
                }
                self.touch();
                self.session_changed();
                self.toast(ToastTone::Neutral, "Request deleted");
            }
            SideAction::RenameProject => {
                self.rename = Some(RenameState {
                    target: RenameTarget::Project,
                    text: self.data.name.clone(),
                });
            }
            SideAction::CopyCurl(id) => {
                let code = self.data.find_request(id).map(|req| {
                    let env = self.data.env_map();
                    let mut request = req.clone();
                    if let Some(auth) = self.data.resolved_auth(id) {
                        request.auth = auth;
                    }
                    codegen::curl_for(&request, &env)
                });
                if let Some(code) = code {
                    self.pending_copy = Some(code);
                    self.toast(ToastTone::Success, "cURL copied to clipboard");
                }
            }
            SideAction::OpenSnippets(id) => {
                self.snippets = Some(SnippetsState {
                    request_id: id,
                    lang: codegen::Lang::Curl,
                    resolved: true,
                });
            }
            SideAction::FolderAuth(id) => {
                if let Some(folder) = self.data.folders.iter().find(|f| f.id == id) {
                    let auth = if matches!(folder.auth, Auth::Inherit) {
                        Auth::None
                    } else {
                        folder.auth.clone()
                    };
                    self.folder_auth = Some(FolderAuthState {
                        folder_id: id,
                        name: folder.name.clone(),
                        auth,
                    });
                }
            }
            SideAction::ImportCurl(folder) => {
                let folder = folder.or_else(|| {
                    self.active_tab
                        .and_then(|id| self.data.folder_of(id))
                        .map(|f| f.id)
                        .or_else(|| self.data.folders.first().map(|f| f.id))
                });
                self.import = Some(ImportState {
                    text: String::new(),
                    folder,
                    error: None,
                });
            }
        }
    }

    fn open_request(&mut self, id: Id) {
        if !self.open_tabs.contains(&id) {
            self.open_tabs.push(id);
        }
        self.active_tab = Some(id);
        self.nav = Nav::Apis;
        self.session_changed();
    }

    fn close_tab(&mut self, id: Id) {
        self.open_tabs.retain(|t| *t != id);
        if self.active_tab == Some(id) {
            self.active_tab = self.open_tabs.first().copied();
        }
        self.session_changed();
    }

    fn start_send(&mut self, mut req: ApiRequest) {
        if let Some(auth) = self.data.resolved_auth(req.id) {
            req.auth = auth;
        }
        let env = self.data.env_map();
        let settings = self.data.settings.clone();
        let cache = self.oauth.clone();
        let folder = self
            .data
            .folder_of(req.id)
            .map(|f| f.name.clone())
            .unwrap_or_default();
        let request_id = req.id;
        let handle = self.runtime.spawn(async move {
            let mut outcome = http::execute_async(&req, &env, &settings, &cache).await;
            outcome.folder = folder;
            outcome
        });
        self.sending = Some(request_id);
        self.task = Some(handle);
    }

    fn handle_native_menu(&mut self) {
        use crate::menu::MenuAction;
        while let Some(action) = crate::menu::poll() {
            match action {
                MenuAction::NewRequest => {
                    let request =
                        ApiRequest::new("New request", Method::Get, "https://api.example.com");
                    let request_id = request.id;
                    match self.data.folders.first_mut() {
                        Some(folder) => {
                            folder.open = true;
                            folder.requests.push(request);
                        }
                        None => {
                            let mut folder = Folder::new("Requests");
                            folder.requests.push(request);
                            self.data.folders.push(folder);
                        }
                    }
                    self.open_request(request_id);
                    self.touch();
                }
                MenuAction::Save => {
                    self.save_now();
                    self.toast(ToastTone::Success, "Workspace saved");
                }
                MenuAction::ImportCurl => {
                    self.import = Some(ImportState {
                        text: String::new(),
                        folder: self.data.folders.first().map(|folder| folder.id),
                        error: None,
                    });
                }
                MenuAction::ImportApi => {
                    self.api_import = Some(ApiImportState::default());
                }
                MenuAction::Export => {
                    self.export = Some(ExportState {
                        format: exporter::Format::OpenApi,
                        doc: exporter::export(&self.data, exporter::Format::OpenApi),
                        saved: None,
                    });
                }
                MenuAction::PublishDocs => self.open_publish(),
                MenuAction::Settings => {
                    self.nav = Nav::Settings;
                }
                MenuAction::Send => {
                    if self.sending.is_none() {
                        if let Some(id) = self.active_tab {
                            if let Some(request) = self.data.find_request(id) {
                                let request = request.clone();
                                self.start_send(request);
                            }
                        }
                    }
                }
                MenuAction::CopyCurl => {
                    if let Some(id) = self.active_tab {
                        if let Some(request) = self.data.find_request(id) {
                            let mut request = request.clone();
                            if let Some(auth) = self.data.resolved_auth(id) {
                                request.auth = auth;
                            }
                            let env = self.data.env_map();
                            self.pending_copy = Some(codegen::curl_for(&request, &env));
                        }
                    }
                }
                MenuAction::Snippets => {
                    if let Some(id) = self.active_tab {
                        self.snippets = Some(SnippetsState {
                            request_id: id,
                            lang: codegen::Lang::Curl,
                            resolved: true,
                        });
                    }
                }
                MenuAction::Undo => self.undo(),
                MenuAction::Redo => self.redo(),
            }
        }
    }

    fn poll_http(&mut self) {
        let Some(handle) = &self.task else { return };
        if !handle.is_finished() {
            return;
        }
        let handle = self.task.take().expect("task");
        self.sending = None;
        match self.runtime.block_on(handle) {
            Ok(outcome) => self.handle_outcome(outcome),
            Err(error) if error.is_cancelled() => {}
            Err(error) => self.toast(
                ToastTone::Danger,
                format!("The request stopped unexpectedly: {error}"),
            ),
        }
    }

    fn cancel_send(&mut self) {
        let Some(handle) = self.task.take() else { return };
        handle.abort();
        self.sending = None;
        self.toast(ToastTone::Neutral, "Request cancelled");
    }

    fn handle_outcome(&mut self, outcome: RequestOutcome) {
        let (status, ms, size) = match &outcome.data {
            Ok(data) => (Some(data.status), data.elapsed_ms, data.size),
            Err(_) => (None, 0, 0),
        };
        self.responses.insert(
            outcome.request_id,
            Arc::new(StoredResponse {
                data: outcome.data.clone(),
                assertions: outcome.assertions.clone(),
                at: now_secs(),
            }),
        );
        self.data.add_history(HistoryEntry {
            id: crate::model::next_id(),
            request_id: Some(outcome.request_id),
            method: outcome.method,
            url: outcome.url.clone(),
            status,
            ms,
            size: size as u64,
            at: outcome.at,
        });
        self.touch_ui();
        let captured = self.capture_into_env(std::slice::from_ref(&outcome));
        match status {
            Some(code) => {
                let mut message = format!("{code} received in {ms} ms");
                if !captured.is_empty() {
                    message.push_str(&format!(" · captured {}", captured.join(", ")));
                }
                self.toast(
                    if (200..400).contains(&code) {
                        ToastTone::Success
                    } else {
                        ToastTone::Neutral
                    },
                    message,
                );
            }
            None => self.toast(ToastTone::Danger, "The request could not be completed"),
        }
    }

    /// Writes the values extracted by each request's capture rules into the
    /// active environment. Returns the variable names that were updated.
    fn capture_into_env(&mut self, outcomes: &[RequestOutcome]) -> Vec<String> {
        let mut values: Vec<(String, String, bool)> = Vec::new();
        for outcome in outcomes {
            let Ok(data) = &outcome.data else {
                continue;
            };
            let Some(request) = self.data.find_request(outcome.request_id) else {
                continue;
            };
            values.extend(http::evaluate_captures(&request.captures, data));
        }
        if values.is_empty() {
            return Vec::new();
        }
        let env_id = match self.data.active_env {
            Some(id) if self.data.envs.iter().any(|env| env.id == id) => id,
            _ => {
                let mut env = crate::model::Environment::new("Captured");
                env.vars.clear();
                let id = env.id;
                self.data.envs.push(env);
                self.data.active_env = Some(id);
                self.env_selected = Some(id);
                id
            }
        };
        let mut names: Vec<String> = Vec::new();
        for (variable, value, secret) in values {
            if let Some(env) = self.data.envs.iter_mut().find(|env| env.id == env_id) {
                match env.vars.iter_mut().find(|var| var.key == variable) {
                    Some(existing) => {
                        existing.value = value;
                        existing.secret = secret;
                        existing.enabled = true;
                    }
                    None => env.vars.push(KV {
                        enabled: true,
                        key: variable.clone(),
                        value,
                        desc: String::new(),
                        secret,
                    }),
                }
            }
            if !names.contains(&variable) {
                names.push(variable);
            }
        }
        self.touch_ui();
        names
    }

    fn start_test_run(&mut self) {
        let mut env = self.data.env_map();
        let settings = self.data.settings.clone();
        let cache = self.oauth.clone();
        let mut requests: Vec<(String, ApiRequest)> = Vec::new();
        for folder in &self.data.folders {
            for req in &folder.requests {
                let mut request = req.clone();
                request.auth = request.auth.resolve(&folder.auth);
                requests.push((folder.name.clone(), request));
            }
        }
        if requests.is_empty() {
            self.toast(ToastTone::Neutral, "There are no requests to test yet");
            return;
        }
        self.test_running = true;
        self.test_results.clear();
        let handle = self.runtime.spawn(async move {
            let started = Instant::now();
            let mut out = Vec::new();
            for (folder, req) in requests {
                let mut outcome = http::execute_async(&req, &env, &settings, &cache).await;
                outcome.folder = folder;
                // Captures feed the next request in the same run, so chains
                // such as "create session -> refresh token" work unattended.
                if let Ok(data) = &outcome.data {
                    for (variable, value, _) in http::evaluate_captures(&req.captures, data) {
                        env.insert(variable, value);
                    }
                }
                out.push(outcome);
            }
            let elapsed = started.elapsed().as_millis() as u64;
            (out, elapsed)
        });
        self.test_task = Some(handle);
    }

    fn poll_tests(&mut self) {
        let Some(handle) = &self.test_task else { return };
        if !handle.is_finished() {
            return;
        }
        let handle = self.test_task.take().expect("test task");
        self.test_running = false;
        let received = self.runtime.block_on(handle).ok();
        if let Some((results, elapsed)) = received {
            self.test_duration_ms = elapsed;
            let failed: usize = results.iter().map(|r| r.failed()).sum();
            let passed: usize = results.iter().map(|r| r.passed()).sum();
            self.test_results = results;
            let captured = {
                let results = std::mem::take(&mut self.test_results);
                let names = self.capture_into_env(&results);
                self.test_results = results;
                names
            };
            let mut message = format!("Tests finished · {passed} passed, {failed} failed");
            if !captured.is_empty() {
                message.push_str(&format!(" · captured {}", captured.join(", ")));
            }
            self.toast(
                if failed == 0 {
                    ToastTone::Success
                } else {
                    ToastTone::Neutral
                },
                message,
            );
        }
    }

    fn mock_routes(&self) -> Vec<MockRoute> {
        let env = self.data.env_map();
        self.data
            .all_requests()
            .into_iter()
            .filter(|r| r.mock)
            .map(|r| {
                let url = http::resolved_url(r, &env);
                let path = reqwest::Url::parse(&url)
                    .map(|u| u.path().to_string())
                    .unwrap_or_else(|_| r.url.clone());
                MockRoute {
                    method: r.method,
                    path,
                    status: if r.example_status == 0 {
                        200
                    } else {
                        r.example_status
                    },
                    body: r.example_body.clone(),
                    latency_ms: r.mock_latency_ms,
                    failure: r.mock_failure,
                }
            })
            .collect()
    }

    fn sync_mock(&mut self) {
        if self.mock.running && self.mock_dirty {
            let port = self.data.settings.mock_port;
            self.mock.start(port, self.mock_routes());
            self.mock_dirty = false;
        }
    }

    // -----------------------------------------------------------------------
    // Window chrome
    // -----------------------------------------------------------------------

    fn top_bar(&mut self, ui: &mut Ui, p: &Palette) {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 9.0;
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::hover());
            widgets::logo(ui.painter(), rect);
            widgets::wordmark(ui, p, 16.0);

            if let Some(id) = self.active_tab {
                if let Some(req) = self.data.find_request(id) {
                    let folder = self
                        .data
                        .folder_of(id)
                        .map(|f| f.name.clone())
                        .unwrap_or_default();
                    ui.add_space(4.0);
                    ui.label(RichText::new("/").size(12.5).color(p.faint));
                    ui.label(RichText::new(folder).size(12.5).color(p.muted));
                    ui.label(RichText::new("/").size(12.5).color(p.faint));
                    ui.label(RichText::new(&req.name).size(12.5).color(p.text));
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let theme_icon = if self.data.settings.dark {
                    Icon::Sun
                } else {
                    Icon::Moon
                };
                if widgets::icon_button(ui, p, theme_icon, 28.0, "Switch theme").clicked() {
                    self.data.settings.dark = !self.data.settings.dark;
                    let palette = self.palette();
                    theme::apply_style(ui.ctx(), &palette);
                    self.touch_ui();
                }
                if widgets::icon_button(ui, p, Icon::Gear, 28.0, "Settings").clicked() {
                    self.nav = Nav::Settings;
                }
                ui.add_space(4.0);
                let names: Vec<(Id, String)> = self
                    .data
                    .envs
                    .iter()
                    .map(|e| (e.id, e.name.clone()))
                    .collect();
                let selected = self
                    .data
                    .active_env()
                    .map(|e| e.name.clone())
                    .unwrap_or_else(|| "No environment".to_string());
                egui::ComboBox::from_id_salt("env_picker")
                    .width(138.0)
                    .selected_text(RichText::new(selected).size(12.0).color(p.text))
                    .show_ui(ui, |ui| {
                        for (id, name) in &names {
                            let active = self.data.active_env == Some(*id);
                            if ui
                                .selectable_label(active, RichText::new(name).size(12.0))
                                .clicked()
                            {
                                self.data.active_env = Some(*id);
                                self.env_selected = Some(*id);
                                self.touch();
                            }
                        }
                        ui.separator();
                        if ui.button("Manage environments").clicked() {
                            self.nav = Nav::Environments;
                        }
                    });
                ui.add_space(4.0);
                ui.label(RichText::new("Env").size(11.0).color(p.faint));
                ui.add_space(2.0);
                let dot_color = if self.data.active_env.is_some() {
                    p.success
                } else {
                    p.faint
                };
                let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                ui.painter()
                    .circle_filled(dot_rect.center(), 3.5, dot_color);
            });
        });
        let rect = ui.max_rect();
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0_f32, p.border),
        );
    }

    fn rail(&mut self, ui: &mut Ui, p: &Palette) {
        ui.add_space(10.0);
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            for nav in [
                Nav::Apis,
                Nav::History,
                Nav::Environments,
                Nav::Tests,
                Nav::Mock,
                Nav::Docs,
            ] {
                self.rail_item(ui, p, nav);
            }
        });

        let remaining = ui.available_height() - 52.0;
        if remaining > 0.0 {
            ui.add_space(remaining);
        }
        ui.vertical_centered(|ui| {
            self.rail_item(ui, p, Nav::Settings);
        });
        ui.add_space(8.0);
        let rect = ui.max_rect();
        ui.painter().vline(
            rect.right() - 0.5,
            rect.y_range(),
            Stroke::new(1.0_f32, p.border),
        );
    }

    fn rail_item(&mut self, ui: &mut Ui, p: &Palette, nav: Nav) {
        let active = self.nav == nav;
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(38.0, 38.0), Sense::click());
        if active {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(9), p.accent_soft);
            let bar = egui::Rect::from_min_size(
                pos2(rect.left() - 9.0, rect.center().y - 9.0),
                vec2(3.0, 18.0),
            );
            ui.painter()
                .rect_filled(bar, CornerRadius::same(2), p.accent);
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(9), p.hover);
        }
        let icon_rect = egui::Rect::from_center_size(rect.center(), Vec2::splat(19.0));
        widgets::paint_icon(
            ui.painter(),
            icon_rect,
            nav.icon(),
            if active { p.accent } else { p.muted },
            1.5,
        );
        if resp.clicked() {
            self.nav = nav;
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        resp.on_hover_text(nav.label());
    }

    fn sidebar(&mut self, ui: &mut Ui, p: &Palette) {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - 4.0);
                self.project_picker(ui, p);
                ui.add_space(8.0);
                self.search_box(ui, p);
                ui.add_space(10.0);
            });
        });

        ui.horizontal(|ui| {
            ui.add_space(14.0);
            widgets::section_label(ui, p, "APIS");
            ui.label(
                RichText::new(self.data.total_requests().to_string())
                    .size(10.0)
                    .color(p.faint),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(8.0);
                if widgets::icon_button(ui, p, Icon::Plus, 22.0, "New request").clicked() {
                    self.apply_side_action(SideAction::NewRequest(None));
                }
                if widgets::icon_button(ui, p, Icon::Folder, 22.0, "New collection").clicked() {
                    self.apply_side_action(SideAction::NewFolder);
                }
            });
        });
        ui.add_space(2.0);

        let mut action: Option<SideAction> = None;
        let query = self.search.trim().to_lowercase();

        #[derive(Clone, Copy)]
        enum SideRow {
            Folder(usize),
            Request(usize, usize),
        }

        let mut rows: Vec<SideRow> = Vec::new();
        let mut matching = 0usize;
        for (folder_index, folder) in self.data.folders.iter().enumerate() {
            rows.push(SideRow::Folder(folder_index));
            if !folder.open {
                continue;
            }
            for (request_index, request) in folder.requests.iter().enumerate() {
                let matches = query.is_empty()
                    || request.name.to_lowercase().contains(&query)
                    || request.url.to_lowercase().contains(&query);
                if matches {
                    matching += 1;
                    rows.push(SideRow::Request(folder_index, request_index));
                }
            }
        }

        ScrollArea::vertical()
            .id_salt("tree_scroll")
            .auto_shrink([false, false])
            .show_rows(ui, 30.0, rows.len(), |ui, range| {
                for row in &rows[range] {
                    match *row {
                        SideRow::Folder(folder_index) => {
                            let folder = &self.data.folders[folder_index];
                            let contains_active = self
                                .active_tab
                                .map(|id| folder.requests.iter().any(|request| request.id == id))
                                .unwrap_or(false);
                            if let Some(a) = self.folder_row(ui, p, folder, contains_active) {
                                action = Some(a);
                            }
                        }
                        SideRow::Request(folder_index, request_index) => {
                            let request = &self.data.folders[folder_index].requests[request_index];
                            let selected = self.active_tab == Some(request.id);
                            if let Some(a) = self.request_row(ui, p, request, selected) {
                                action = Some(a);
                            }
                        }
                    }
                }
            });
        if !query.is_empty() && matching == 0 {
            ui.add_space(4.0);
            ui.label(
                RichText::new("No requests match your search.")
                    .size(11.0)
                    .color(p.faint),
            );
        }
        ui.add_space(8.0);

        if let Some(a) = action {
            self.apply_side_action(a);
        }

        ui.horizontal(|ui| {
            ui.add_space(14.0);
            let (dot, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
            ui.painter().circle_filled(dot.center(), 3.0, p.success);
            ui.label(RichText::new("Local workspace").size(10.5).color(p.faint));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(12.0);
                ui.label(RichText::new("⌘↵").size(10.0).color(p.faint));
            });
        });
        ui.add_space(8.0);
        let rect = ui.max_rect();
        ui.painter().vline(
            rect.right() - 0.5,
            rect.y_range(),
            Stroke::new(1.0_f32, p.border),
        );
    }

    fn project_picker(&mut self, ui: &mut Ui, p: &Palette) {
        let name = self.data.name.clone();
        let mut action: Option<SideAction> = None;
        let mut reset = false;
        let mut export = false;
        let mut import = false;
        let mut import_api = false;
        let mut export_dialog = false;
        let mut publish_docs = false;
        let mut undo = false;
        let mut redo = false;
        let can_undo = self.history.can_undo();
        let can_redo = self.history.can_redo();
        egui::ComboBox::from_id_salt("project_picker")
            .width(ui.available_width())
            .height(34.0)
            .selected_text(RichText::new(name).size(13.0).family(theme::medium_family()).color(p.text))
            .show_ui(ui, |ui| {
                ui.set_min_width(210.0);
                if ui
                    .add_enabled(can_undo, egui::Button::new("Undo  ⌘Z"))
                    .clicked()
                {
                    undo = true;
                }
                if ui
                    .add_enabled(can_redo, egui::Button::new("Redo  ⇧⌘Z"))
                    .clicked()
                {
                    redo = true;
                }
                ui.separator();
                if ui.button("Rename workspace").clicked() {
                    action = Some(SideAction::RenameProject);
                }
                if ui.button("Import from cURL…").clicked() {
                    import = true;
                }
                if ui.button("Import OpenAPI / Postman…").clicked() {
                    import_api = true;
                }
                if ui.button("Export as OpenAPI / Postman…").clicked() {
                    export_dialog = true;
                }
                if ui.button("Publish documentation…").clicked() {
                    publish_docs = true;
                }
                if ui.button("Save workspace now").clicked() {
                    export = true;
                }
                ui.separator();
                if ui
                    .button(RichText::new("Reset demo data").color(p.danger))
                    .clicked()
                {
                    reset = true;
                }
            });
        if undo {
            self.undo();
            return;
        }
        if redo {
            self.redo();
            return;
        }
        if let Some(a) = action {
            self.apply_side_action(a);
        }
        if import {
            self.apply_side_action(SideAction::ImportCurl(None));
        }
        if import_api {
            self.api_import = Some(ApiImportState::default());
        }
        if publish_docs {
            self.open_publish();
        }
        if export_dialog {
            let format = ExportFormat::OpenApi;
            self.export = Some(ExportState {
                format,
                doc: exporter::export(&self.data, format),
                saved: None,
            });
        }
        if export {
            let path = crate::model::storage_path();
            match crate::model::save(&self.data) {
                Ok(()) => self.toast(
                    ToastTone::Success,
                    format!("Workspace saved to {}", path.display()),
                ),
                Err(err) => self.toast(ToastTone::Danger, format!("Export failed: {err}")),
            }
        }
        if reset {
            self.confirm_reset = true;
        }
    }

    fn search_box(&mut self, ui: &mut Ui, p: &Palette) {
        Frame::NONE
            .fill(p.panel_alt)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(7))
            .inner_margin(Margin::symmetric(8, 5))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                    widgets::paint_icon(ui.painter(), icon_rect, Icon::Search, p.faint, 1.3);
                    let width = ui.available_width() - 4.0;
                    ui.add_sized(
                        [width, 20.0],
                        TextEdit::singleline(&mut self.search)
                            .frame(false)
                            .hint_text(RichText::new("Search requests").size(12.0).color(p.faint))
                            .font(FontId::proportional(12.0))
                            .margin(Margin::symmetric(0, 2)),
                    );
                });
            });
    }

    fn folder_row(
        &self,
        ui: &mut Ui,
        p: &Palette,
        folder: &Folder,
        contains_active: bool,
    ) -> Option<SideAction> {
        let width = ui.available_width() - 8.0;
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 26.0), Sense::click());
        let hovered = resp.hovered();
        if hovered {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(6), p.hover);
        }
        let chevron = egui::Rect::from_center_size(
            pos2(rect.left() + 16.0, rect.center().y),
            Vec2::splat(12.0),
        );
        widgets::paint_icon(
            ui.painter(),
            chevron,
            if folder.open {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            },
            p.faint,
            1.3,
        );
        let folder_icon = egui::Rect::from_center_size(
            pos2(rect.left() + 34.0, rect.center().y),
            Vec2::splat(13.0),
        );
        widgets::paint_icon(
            ui.painter(),
            folder_icon,
            Icon::Folder,
            if contains_active { p.accent } else { p.muted },
            1.3,
        );
        let name_font = theme::medium(12.0);
        let available = rect.width() - 78.0;
        let label = elide(ui.painter(), &folder.name, &name_font, available);
        ui.painter().text(
            pos2(rect.left() + 46.0, rect.center().y + 0.5),
            Align2::LEFT_CENTER,
            label,
            name_font,
            p.text,
        );
        ui.painter().text(
            pos2(rect.right() - 12.0, rect.center().y + 0.5),
            Align2::RIGHT_CENTER,
            folder.requests.len().to_string(),
            FontId::proportional(10.0),
            p.faint,
        );
        if hovered {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let id = folder.id;
        let mut action: Option<SideAction> = None;
        resp.clone().context_menu(|ui| {
            if ui.button("New request").clicked() {
                action = Some(SideAction::NewRequest(Some(id)));
                ui.close_menu();
            }
            if ui.button("Import from cURL…").clicked() {
                action = Some(SideAction::ImportCurl(Some(id)));
                ui.close_menu();
            }
            if ui.button("Collection auth…").clicked() {
                action = Some(SideAction::FolderAuth(id));
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Rename collection").clicked() {
                action = Some(SideAction::RenameFolder(id));
                ui.close_menu();
            }
            if ui
                .button(RichText::new("Delete collection").color(p.danger))
                .clicked()
            {
                action = Some(SideAction::DeleteFolder(id));
                ui.close_menu();
            }
        });
        if resp.clicked() {
            action = Some(SideAction::ToggleFolder(id));
        }
        action
    }

    fn request_row(
        &self,
        ui: &mut Ui,
        p: &Palette,
        req: &ApiRequest,
        selected: bool,
    ) -> Option<SideAction> {
        let width = ui.available_width() - 8.0;
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 28.0), Sense::click());
        let hovered = resp.hovered();
        if selected {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(6), p.accent_soft);
            let bar = egui::Rect::from_min_size(
                pos2(rect.left(), rect.center().y - 8.0),
                vec2(2.5, 16.0),
            );
            ui.painter()
                .rect_filled(bar, CornerRadius::same(2), p.accent);
        } else if hovered {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(6), p.hover);
        }

        let badge_rect = egui::Rect::from_min_size(
            pos2(rect.left() + 18.0, rect.center().y - 9.0),
            vec2(44.0, 18.0),
        );
        let color = p.method_color(req.method);
        ui.painter().rect_filled(
            badge_rect,
            CornerRadius::same(4),
            theme::mix(color, p.panel, if selected { 0.84 } else { 0.9 }),
        );
        ui.painter().text(
            badge_rect.center() + vec2(0.0, 0.5),
            Align2::CENTER_CENTER,
            req.method.as_str(),
            FontId::new(8.8, theme::mono_medium_family()),
            color,
        );

        let name_font = FontId::proportional(12.0);
        let available = rect.width() - 118.0;
        let label = elide(ui.painter(), &req.name, &name_font, available);
        ui.painter().text(
            pos2(rect.left() + 70.0, rect.center().y + 0.5),
            Align2::LEFT_CENTER,
            label,
            name_font,
            if selected { p.accent } else { p.text },
        );

        if req.mock {
            let dot = pos2(rect.right() - 32.0, rect.center().y);
            ui.painter().circle_filled(dot, 2.5, p.warn);
        }
        ui.painter().text(
            pos2(rect.right() - 12.0, rect.center().y + 0.5),
            Align2::RIGHT_CENTER,
            "›",
            FontId::proportional(13.0),
            if hovered || selected {
                p.muted
            } else {
                p.faint
            },
        );
        if hovered {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }

        let id = req.id;
        let mut action: Option<SideAction> = None;
        resp.clone().context_menu(|ui| {
            if ui.button("Copy as cURL").clicked() {
                action = Some(SideAction::CopyCurl(id));
                ui.close_menu();
            }
            if ui.button("Code snippets…").clicked() {
                action = Some(SideAction::OpenSnippets(id));
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Rename request").clicked() {
                action = Some(SideAction::RenameRequest(id));
                ui.close_menu();
            }
            if ui
                .button(RichText::new("Delete request").color(p.danger))
                .clicked()
            {
                action = Some(SideAction::DeleteRequest(id));
                ui.close_menu();
            }
        });
        if resp.clicked() {
            action = Some(SideAction::Open(id));
        }
        action
    }

    // -----------------------------------------------------------------------
    // API workspace
    // -----------------------------------------------------------------------

    fn tab_bar(&mut self, ui: &mut Ui, p: &Palette) {
        let mut close: Option<Id> = None;
        let mut select: Option<Id> = None;
        let mut new_request = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            ui.add_space(6.0);
            for id in self.open_tabs.clone() {
                let Some(req) = self.data.find_request(id) else {
                    continue;
                };
                let selected = self.active_tab == Some(id);
                let label_font = if selected {
                    theme::medium(12.0)
                } else {
                    FontId::proportional(12.0)
                };
                let name = elide(ui.painter(), &req.name, &label_font, 150.0);
                let text_width = ui
                    .painter()
                    .layout_no_wrap(name.clone(), label_font.clone(), Color32::WHITE)
                    .size()
                    .x;
                let width = (text_width + 62.0).clamp(110.0, 208.0);
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 34.0), Sense::click());
                if selected {
                    ui.painter().rect_filled(
                        rect,
                        CornerRadius {
                            nw: 7,
                            ne: 7,
                            sw: 0,
                            se: 0,
                        },
                        p.panel,
                    );
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(rect.left_top(), vec2(rect.width(), 2.0)),
                        CornerRadius::ZERO,
                        p.accent,
                    );
                } else if resp.hovered() {
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(6), p.hover);
                }
                ui.painter().circle_filled(
                    pos2(rect.left() + 16.0, rect.center().y),
                    3.0,
                    p.method_color(req.method),
                );
                ui.painter().text(
                    pos2(rect.left() + 26.0, rect.center().y + 0.5),
                    Align2::LEFT_CENTER,
                    &name,
                    label_font,
                    if selected { p.text } else { p.muted },
                );
                let close_rect = egui::Rect::from_center_size(
                    pos2(rect.right() - 15.0, rect.center().y),
                    Vec2::splat(16.0),
                );
                let close_resp =
                    ui.interact(close_rect, ui.id().with(("tab_close", id)), Sense::click());
                if close_resp.hovered() {
                    ui.painter().rect_filled(
                        close_rect.shrink(1.0),
                        CornerRadius::same(4),
                        p.hover,
                    );
                }
                widgets::paint_icon(
                    ui.painter(),
                    close_rect.shrink(4.0),
                    Icon::Close,
                    if close_resp.hovered() {
                        p.text
                    } else {
                        p.faint
                    },
                    1.3,
                );
                if close_resp.clicked() {
                    close = Some(id);
                } else if resp.clicked() {
                    select = Some(id);
                }
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                ui.add_space(2.0);
            }
            if widgets::icon_button(ui, p, Icon::Plus, 26.0, "New request").clicked() {
                new_request = true;
            }
        });
        if let Some(id) = select {
            self.active_tab = Some(id);
            self.session_changed();
        }
        if let Some(id) = close {
            self.close_tab(id);
        }
        if new_request {
            self.apply_side_action(SideAction::NewRequest(None));
        }
        let rect = ui.max_rect();
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0_f32, p.border),
        );
        ui.add_space(1.0);
    }

    fn api_view(&mut self, ui: &mut Ui, p: &Palette) {
        Frame::NONE.fill(p.panel).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            Frame::NONE.fill(p.panel_alt).show(ui, |ui| {
                self.tab_bar(ui, p);
            });
            if let Some(id) = self.active_tab {
                if self.data.find_request(id).is_some() {
                    self.workspace(ui, p, id);
                    return;
                }
                self.active_tab = None;
            }
            self.welcome(ui, p);
        });
    }

    fn welcome(&mut self, ui: &mut Ui, p: &Palette) {
        let height = ui.available_height();
        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), height),
            Layout::top_down(Align::Center),
            |ui| {
                ui.add_space((height * 0.22).clamp(40.0, 160.0));
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(56.0), Sense::hover());
                widgets::logo(ui.painter(), rect);
                ui.add_space(18.0);
                ui.label(
                    RichText::new("No request open")
                        .family(theme::display_family())
                        .size(21.0)
                        .extra_letter_spacing(-0.2)
                        .color(p.text),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "Pick a request from the collection on the left, or start a new one.",
                    )
                    .size(12.5)
                    .color(p.muted),
                );
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    let total = ui.available_width();
                    ui.add_space((total / 2.0 - 140.0).max(0.0));
                    if widgets::button(ui, p, "New request", Btn::Primary).clicked() {
                        self.apply_side_action(SideAction::NewRequest(None));
                    }
                    if widgets::button(ui, p, "Browse docs", Btn::Secondary).clicked() {
                        self.nav = Nav::Docs;
                    }
                });
            },
        );
    }

    fn workspace(&mut self, ui: &mut Ui, p: &Palette, id: Id) {
        let Some(mut req) = self.data.find_request(id).cloned() else {
            return;
        };
        let mut changed = false;
        let mut send_now = false;

        // URL bar -----------------------------------------------------------
        Frame::NONE
            .inner_margin(Margin {
                left: 16,
                right: 16,
                top: 10,
                bottom: 8,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let method_color = p.method_color(req.method);
                    egui::ComboBox::from_id_salt(("method_pick", id))
                        .width(86.0)
                        .height(32.0)
                        .selected_text(
                            RichText::new(req.method.as_str())
                                .size(12.0)
                                .family(theme::medium_family())
                                .color(method_color),
                        )
                        .show_ui(ui, |ui| {
                            ui.set_min_width(140.0);
                            for method in Method::ALL {
                                let selected = req.method == method;
                                if ui
                                    .selectable_label(
                                        selected,
                                        RichText::new(method.as_str())
                                            .size(12.0)
                                            .color(p.method_color(method)),
                                    )
                                    .clicked()
                                {
                                    req.method = method;
                                    changed = true;
                                }
                            }
                        });

                    let send_w = 96.0;
                    let save_w = 66.0;
                    let code_w = 28.0;
                    let menu_w = 34.0;
                    let url_w =
                        (ui.available_width() - send_w - save_w - code_w - menu_w - 42.0)
                            .max(160.0);
                    let resolved = self.data.expand(&req.url);
                    Frame::NONE
                        .fill(p.panel_alt)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width(url_w - 22.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                ui.label(RichText::new("URL").size(9.5).color(p.faint).family(theme::medium_family()));
                                let text_w = ui.available_width();
                                let defined = self.defined_vars();
                                let edit = widgets::var_singleline(
                                    ui,
                                    p,
                                    &defined,
                                    &mut req.url,
                                    FontId::monospace(12.5),
                                    "Enter request URL",
                                    [text_w, 21.0],
                                    false,
                                    Margin::ZERO,
                                    egui::Id::new("request_url_field"),
                                );
                                if edit.on_hover_text(resolved).changed() {
                                    changed = true;
                                }
                            });
                        });
                    let loading = self.sending == Some(id);
                    if send_button(ui, p, loading).clicked() && !loading {
                        send_now = true;
                    }
                    if loading {
                        if widgets::button(ui, p, "Cancel", Btn::Secondary).clicked() {
                            self.cancel_send();
                        }
                    } else if widgets::button(ui, p, "Save", Btn::Secondary).clicked() {
                        self.save_now();
                        self.toast(ToastTone::Success, "Workspace saved");
                    }
                    if widgets::icon_button(ui, p, Icon::Code, 28.0, "Code snippets").clicked() {
                        self.snippets = Some(SnippetsState {
                            request_id: id,
                            lang: codegen::Lang::Curl,
                            resolved: true,
                        });
                    }
                    ui.menu_button(RichText::new("···").size(14.0).color(p.muted), |ui| {
                        ui.set_min_width(190.0);
                        if ui.button("Copy as cURL").clicked() {
                            let env = self.data.env_map();
                            ui.ctx().copy_text(codegen::curl_for(&req, &env));
                            self.toast(ToastTone::Success, "cURL copied to clipboard");
                            ui.close_menu();
                        }
                        if ui.button("Code snippets…").clicked() {
                            self.snippets = Some(SnippetsState {
                                request_id: id,
                                lang: codegen::Lang::Curl,
                                resolved: true,
                            });
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Duplicate request").clicked() {
                            let mut copy = req.clone();
                            copy.id = crate::model::next_id();
                            copy.name = format!("{} copy", req.name);
                            let folder_id = self.data.folder_of(id).map(|f| f.id);
                            if let Some(fid) = folder_id {
                                if let Some(folder) =
                                    self.data.folders.iter_mut().find(|f| f.id == fid)
                                {
                                    folder.requests.push(copy);
                                }
                            }
                            self.touch();
                            ui.close_menu();
                        }
                        let mock_label = if req.mock {
                            "Stop mocking this route"
                        } else {
                            "Mock this route"
                        };
                        if ui.button(mock_label).clicked() {
                            req.mock = !req.mock;
                            changed = true;
                            self.mock_dirty = true;
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .button(RichText::new("Delete request").color(p.danger))
                            .clicked()
                        {
                            self.apply_side_action(SideAction::DeleteRequest(id));
                            ui.close_menu();
                        }
                    });
                });
            });

        // Request tabs ------------------------------------------------------
        Frame::NONE.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                let params = format!("Params{}", count_suffix(req.filled_params()));
                let headers = format!("Headers{}", count_suffix(req.filled_headers()));
                let tests = format!("Tests{}", count_suffix(req.assertions.len()));
                let captures = format!("Capture{}", count_suffix(req.captures.len()));
                for (tab, label) in [
                    (ReqTab::Params, params.as_str()),
                    (ReqTab::Body, "Body"),
                    (ReqTab::Headers, headers.as_str()),
                    (ReqTab::Auth, "Auth"),
                    (ReqTab::Assertions, tests.as_str()),
                    (ReqTab::Capture, captures.as_str()),
                ] {
                    if underline_tab(ui, p, label, self.req_tab == tab) {
                        self.req_tab = tab;
                    }
                }
            });
        });
        let y = ui.cursor().top();
        let x_range = ui.max_rect().x_range();
        ui.painter()
            .hline(x_range, y - 0.5, Stroke::new(1.0_f32, p.border));

        let total_h = ui.available_height();
        let cap = if total_h < 640.0 {
            (total_h * 0.5).max(140.0)
        } else {
            (total_h - 160.0).max(150.0)
        };
        let resp_h = self.response_height.clamp(130.0, cap);
        let req_h = (total_h - resp_h - 8.0).max(110.0);

        // Request editor ----------------------------------------------------
        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), req_h),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_clip_rect(ui.max_rect());
                ScrollArea::vertical()
                    .id_salt(("req_editor", id))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.request_editor(ui, p, &mut req, req_h) {
                            changed = true;
                        }
                    });
            },
        );

        // Splitter ----------------------------------------------------------
        let (split_rect, split_resp) =
            ui.allocate_exact_size(vec2(ui.available_width(), 8.0), Sense::drag());
        if split_resp.dragged() {
            self.response_height =
                (self.response_height - split_resp.drag_delta().y).clamp(150.0, 640.0);
        }
        if split_resp.hovered() || split_resp.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
            ui.painter().hline(
                split_rect.x_range(),
                split_rect.center().y,
                Stroke::new(1.0_f32, p.accent),
            );
        }

        // Response ----------------------------------------------------------
        let remaining = ui.available_height();
        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), remaining),
            Layout::top_down(Align::Min),
            |ui| {
                self.response_panel(ui, p, id, remaining);
            },
        );

        if changed {
            if let Some(target) = self.data.find_request_mut(id) {
                *target = req.clone();
            }
            self.touch();
        }
        if send_now {
            self.start_send(req.clone());
        }
    }

    fn request_editor(
        &mut self,
        ui: &mut Ui,
        p: &Palette,
        req: &mut ApiRequest,
        area_height: f32,
    ) -> bool {
        let mut changed = false;
        Frame::NONE
            .inner_margin(Margin {
                left: 16,
                right: 16,
                top: 12,
                bottom: 16,
            })
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&req.name)
                            .size(14.5)
                            .family(theme::medium_family())
                            .color(p.text),
                    );
                    ui.add_space(2.0);
                    let description = elide(
                        ui.painter(),
                        &req.description,
                        &FontId::proportional(11.5),
                        ui.available_width() - 12.0,
                    );
                    if !description.is_empty() {
                        ui.label(
                            RichText::new(description)
                                .size(11.5)
                                .color(p.faint),
                        );
                    }
                });
                ui.add_space(10.0);
                match self.req_tab {
                    ReqTab::Params => {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Query parameters")
                                    .size(12.5)
                                    .family(theme::medium_family())
                                    .color(p.text),
                            );
                            ui.label(
                                RichText::new("appended to the URL when you send")
                                    .size(11.0)
                                    .color(p.faint),
                            );
                        });
                        ui.add_space(6.0);
                        let defined = self.defined_vars();
                        if widgets::kv_table(
                            ui,
                            p,
                            &defined,
                            &mut req.params,
                            true,
                            "params",
                        ) {
                            changed = true;
                        }
                        if req.params.is_empty() {
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new(
                                    "No parameters yet. Add a row to build the query string.",
                                )
                                .size(11.0)
                                .color(p.faint),
                            );
                        }
                    }
                    ReqTab::Body => {
                        let mut mode_index = body_index(req.body_mode);
                        if widgets::segmented(
                            ui,
                            p,
                            &["none", "JSON", "Text", "Form", "Multipart", "Binary"],
                            &mut mode_index,
                        ) {
                            req.body_mode = match mode_index {
                                1 => BodyMode::Json,
                                2 => BodyMode::Text,
                                3 => BodyMode::Form,
                                4 => BodyMode::Multipart,
                                5 => BodyMode::Binary,
                                _ => BodyMode::None,
                            };
                            changed = true;
                        }
                        ui.add_space(10.0);
                        match req.body_mode {
                            BodyMode::None => {
                                ui.label(
                                    RichText::new("This request has no body.")
                                        .size(12.0)
                                        .color(p.muted),
                                );
                            }
                            BodyMode::Form => {
                                let defined = self.defined_vars();
                                if widgets::kv_table(
                                    ui,
                                    p,
                                    &defined,
                                    &mut req.form,
                                    true,
                                    "form",
                                ) {
                                    changed = true;
                                }
                            }
                            BodyMode::Multipart => {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("multipart/form-data")
                                            .size(11.0)
                                            .color(p.faint),
                                    );
                                    ui.with_layout(
                                        Layout::right_to_left(Align::Center),
                                        |ui| {
                                            if widgets::button(ui, p, "+ File", Btn::Ghost)
                                                .clicked()
                                            {
                                                req.multipart
                                                    .push(FormField::file("", ""));
                                                changed = true;
                                            }
                                            if widgets::button(ui, p, "+ Field", Btn::Ghost)
                                                .clicked()
                                            {
                                                req.multipart
                                                    .push(FormField::text("", ""));
                                                changed = true;
                                            }
                                        },
                                    );
                                });
                                ui.add_space(6.0);
                                let mut remove: Option<usize> = None;
                                let count = req.multipart.len();
                                for (index, field) in req.multipart.iter_mut().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 6.0;
                                        let mut enabled = field.enabled;
                                        if ui
                                            .add(egui::Checkbox::without_text(&mut enabled))
                                            .changed()
                                        {
                                            field.enabled = enabled;
                                            changed = true;
                                        }
                                        if ui
                                            .add_sized(
                                                [150.0, 26.0],
                                                TextEdit::singleline(&mut field.key)
                                                    .hint_text("field")
                                                    .font(FontId::monospace(11.5))
                                                    .margin(Margin::symmetric(6, 4)),
                                            )
                                            .changed()
                                        {
                                            changed = true;
                                        }
                                        let mut kind_index = if field.kind == FieldKind::File {
                                            1
                                        } else {
                                            0
                                        };
                                        egui::ComboBox::from_id_salt(
                                            ("multipart_kind", index),
                                        )
                                        .width(76.0)
                                        .selected_text(
                                            RichText::new(if kind_index == 1 {
                                                "File"
                                            } else {
                                                "Text"
                                            })
                                            .size(11.5),
                                        )
                                        .show_ui(ui, |ui| {
                                            if ui
                                                .selectable_label(kind_index == 0, "Text")
                                                .clicked()
                                            {
                                                kind_index = 0;
                                            }
                                            if ui
                                                .selectable_label(kind_index == 1, "File")
                                                .clicked()
                                            {
                                                kind_index = 1;
                                            }
                                        });
                                        let kind = if kind_index == 1 {
                                            FieldKind::File
                                        } else {
                                            FieldKind::Text
                                        };
                                        if kind != field.kind {
                                            field.kind = kind;
                                            changed = true;
                                        }
                                        let hint = if field.kind == FieldKind::File {
                                            "file path"
                                        } else {
                                            "value"
                                        };
                                        if ui
                                            .add_sized(
                                                [250.0, 26.0],
                                                TextEdit::singleline(&mut field.value)
                                                    .hint_text(hint)
                                                    .font(FontId::monospace(11.5))
                                                    .margin(Margin::symmetric(6, 4)),
                                            )
                                            .changed()
                                        {
                                            changed = true;
                                        }
                                        if field.kind == FieldKind::File {
                                            if widgets::button(ui, p, "Browse…", Btn::Ghost)
                                                .clicked()
                                            {
                                                if let Some(path) =
                                                    rfd::FileDialog::new().pick_file()
                                                {
                                                    field.value =
                                                        path.display().to_string();
                                                    changed = true;
                                                }
                                            }
                                            if ui
                                                .add_sized(
                                                    [120.0, 26.0],
                                                    TextEdit::singleline(
                                                        &mut field.content_type,
                                                    )
                                                    .hint_text("content-type")
                                                    .font(FontId::monospace(10.5))
                                                    .margin(Margin::symmetric(6, 4)),
                                                )
                                                .changed()
                                            {
                                                changed = true;
                                            }
                                        }
                                        if count > 1
                                            && widgets::icon_button(
                                                ui, p, Icon::Trash, 20.0, "Remove",
                                            )
                                            .clicked()
                                        {
                                            remove = Some(index);
                                        }
                                    });
                                }
                                if let Some(index) = remove {
                                    req.multipart.remove(index);
                                    changed = true;
                                }
                                if req.multipart.is_empty() {
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(
                                            "Add text fields and files to build the multipart body.",
                                        )
                                        .size(11.0)
                                        .color(p.faint),
                                    );
                                }
                            }
                            BodyMode::Binary => {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    if ui
                                        .add_sized(
                                            [400.0, 26.0],
                                            TextEdit::singleline(&mut req.binary_path)
                                                .hint_text("/path/to/file.bin")
                                                .font(FontId::monospace(11.5))
                                                .margin(Margin::symmetric(6, 4)),
                                        )
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                    if widgets::button(ui, p, "Browse…", Btn::Ghost)
                                        .clicked()
                                    {
                                        if let Some(path) = rfd::FileDialog::new().pick_file() {
                                            req.binary_path = path.display().to_string();
                                            changed = true;
                                        }
                                    }
                                    if !req.binary_path.trim().is_empty() {
                                        match std::fs::metadata(&req.binary_path) {
                                            Ok(meta) => {
                                                ui.label(
                                                    RichText::new(format!(
                                                        "{}",
                                                        format_bytes(meta.len() as usize)
                                                    ))
                                                    .size(10.5)
                                                    .color(p.muted),
                                                );
                                            }
                                            Err(_) => {
                                                ui.label(
                                                    RichText::new("file not found")
                                                        .size(10.5)
                                                        .color(p.danger),
                                                );
                                            }
                                        }
                                    }
                                });
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(
                                        "The raw bytes of this file are sent as the request body.",
                                    )
                                    .size(11.0)
                                    .color(p.faint),
                                );
                            }
                            BodyMode::Json | BodyMode::Text => {
                                ui.horizontal(|ui| {
                                    if widgets::button(ui, p, "Format", Btn::Ghost).clicked() {
                                        if let Some(pretty) = widgets::pretty_json(&req.body) {
                                            req.body = pretty;
                                            changed = true;
                                        }
                                    }
                                    if widgets::button(ui, p, "Minify", Btn::Ghost).clicked() {
                                        if let Ok(value) =
                                            serde_json::from_str::<serde_json::Value>(&req.body)
                                        {
                                            if let Ok(text) = serde_json::to_string(&value) {
                                                req.body = text;
                                                changed = true;
                                            }
                                        }
                                    }
                                    if req.body_mode == BodyMode::Json {
                                        let valid = req.body.trim().is_empty()
                                            || serde_json::from_str::<serde_json::Value>(&req.body)
                                                .is_ok();
                                        ui.add_space(4.0);
                                        if valid {
                                            widgets::chip(
                                                ui,
                                                p,
                                                "valid JSON",
                                                p.success,
                                                p.success_soft,
                                                false,
                                            );
                                        } else {
                                            widgets::chip(
                                                ui,
                                                p,
                                                "invalid JSON",
                                                p.danger,
                                                p.danger_soft,
                                                false,
                                            );
                                        }
                                    }
                                    ui.with_layout(
                                        Layout::right_to_left(Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(format!(
                                                    "{} chars",
                                                    req.body.chars().count()
                                                ))
                                                .size(10.5)
                                                .color(p.faint),
                                            );
                                        },
                                    );
                                });
                                ui.add_space(6.0);
                                let editor_height =
                                    (area_height - 216.0).clamp(110.0, 420.0);
                                Frame::NONE
                                    .fill(p.code_bg)
                                    .stroke(Stroke::new(1.0_f32, p.border))
                                    .corner_radius(CornerRadius::same(8))
                                    .inner_margin(Margin::same(10))
                                    .show(ui, |ui| {
                                        let defined = self.defined_vars();
                                        if widgets::var_multiline(
                                            ui,
                                            p,
                                            &defined,
                                            &mut req.body,
                                            FontId::monospace(12.0),
                                            "",
                                            [ui.available_width(), editor_height],
                                            Margin::ZERO,
                                            egui::Id::new("request_body_editor"),
                                        )
                                        .changed()
                                        {
                                            changed = true;
                                        }
                                    });
                            }
                        }
                    }
                    ReqTab::Headers => {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Request headers")
                                    .size(12.5)
                                    .family(theme::medium_family())
                                    .color(p.text),
                            );
                            ui.label(
                                RichText::new("Content-Type is set automatically")
                                    .size(11.0)
                                    .color(p.faint),
                            );
                        });
                        ui.add_space(6.0);
                        let defined = self.defined_vars();
                        if widgets::kv_table(
                            ui,
                            p,
                            &defined,
                            &mut req.headers,
                            true,
                            "headers",
                        ) {
                            changed = true;
                        }
                    }
                    ReqTab::Auth => {
                        let folder = self
                            .data
                            .folder_of(req.id)
                            .map(|folder| {
                                (folder.id, folder.name.clone(), folder.auth.clone())
                            });
                        let inherited = matches!(req.auth, Auth::Inherit);
                        let subtitle = if inherited {
                            folder
                                .as_ref()
                                .map(|(_, name, auth)| {
                                    format!("inherited from {name} · {}", auth.label())
                                })
                                .unwrap_or_else(|| {
                                    "inherited · no collection auth".to_string()
                                })
                        } else {
                            "applied to this request".to_string()
                        };
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Authentication")
                                    .size(12.5)
                                    .family(theme::medium_family())
                                    .color(p.text),
                            );
                            ui.label(
                                RichText::new(subtitle).size(11.0).color(p.faint),
                            );
                        });
                        ui.add_space(10.0);
                        self.auth_editor(ui, p, &mut req.auth, true, folder, &mut changed);
                    }
                    ReqTab::Capture => {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Capture variables")
                                    .size(12.5)
                                    .family(theme::medium_family())
                                    .color(p.text),
                            );
                            ui.label(
                                RichText::new("saved to the active environment after every send")
                                    .size(11.0)
                                    .color(p.faint),
                            );
                        });
                        ui.add_space(4.0);
                        let env_name = self
                            .data
                            .active_env
                            .and_then(|id| self.data.envs.iter().find(|e| e.id == id))
                            .map(|e| e.name.clone());
                        ui.horizontal(|ui| {
                            match env_name {
                                Some(name) => {
                                    widgets::chip(
                                        ui,
                                        p,
                                        &format!("Capturing into {name}"),
                                        p.accent,
                                        p.accent_soft,
                                        false,
                                    );
                                }
                                None => {
                                    ui.label(
                                        RichText::new(
                                            "No environment selected: a “Captured” environment will be created.",
                                        )
                                        .size(10.5)
                                        .color(p.warn),
                                    );
                                }
                            }
                        });
                        ui.add_space(8.0);
                        let mut remove: Option<usize> = None;
                        let count = req.captures.len();
                        for (i, capture) in req.captures.iter_mut().enumerate() {
                            let (row_changed, row_removed) =
                                widgets::capture_row(ui, p, i, capture, count > 1);
                            if row_changed {
                                changed = true;
                            }
                            if row_removed {
                                remove = Some(i);
                            }
                        }
                        if let Some(i) = remove {
                            req.captures.remove(i);
                            changed = true;
                        }
                        ui.add_space(2.0);
                        if widgets::button(ui, p, "+ Add capture", Btn::Ghost).clicked() {
                            req.captures.push(Capture::new(CaptureSource::JsonField, "", ""));
                            changed = true;
                        }
                        if req.captures.is_empty() {
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(
                                    "Extract values such as tokens or ids from the response and reuse them as {{variables}} in later requests. You can also right-click a value in the response tree.",
                                )
                                .size(11.0)
                                .color(p.faint),
                            );
                        }
                    }
                    ReqTab::Assertions => {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Assertions")
                                    .size(12.5)
                                    .family(theme::medium_family())
                                    .color(p.text),
                            );
                            ui.label(
                                RichText::new("checked after every send and test run")
                                    .size(11.0)
                                    .color(p.faint),
                            );
                        });
                        ui.add_space(6.0);
                        let mut remove: Option<usize> = None;
                        let count = req.assertions.len();
                        for (i, assertion) in req.assertions.iter_mut().enumerate() {
                            let (row_changed, row_removed) =
                                widgets::assertion_row(ui, p, i, assertion, count > 1);
                            if row_changed {
                                changed = true;
                            }
                            if row_removed {
                                remove = Some(i);
                            }
                        }
                        if let Some(i) = remove {
                            req.assertions.remove(i);
                            changed = true;
                        }
                        ui.add_space(2.0);
                        if widgets::button(ui, p, "+ Add assertion", Btn::Ghost).clicked() {
                            req.assertions.push(Assertion::new(
                                AssertKind::StatusEquals,
                                "",
                                "200",
                            ));
                            changed = true;
                        }
                        if req.assertions.is_empty() {
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(
                                    "No assertions yet. Add one to verify the response automatically.",
                                )
                                .size(11.0)
                                .color(p.faint),
                            );
                        }
                    }
                }
            });
        changed
    }

    fn response_panel(&mut self, ui: &mut Ui, p: &Palette, id: Id, height: f32) {
        let stored = self.responses.get(&id).cloned();
        let req = self.data.find_request(id).cloned().unwrap_or_default();
        let example_body = req.example_body.clone();
        let example_status = if req.example_status == 0 {
            200
        } else {
            req.example_status
        };

        Frame::NONE
            .fill(p.panel)
            .inner_margin(Margin {
                left: 16,
                right: 16,
                top: 8,
                bottom: 10,
            })
            .show(ui, |ui| {
                ui.set_min_height(height - 4.0);
                ui.set_max_height(height - 4.0);
                let mut clear = false;
                let mut copy = false;
                let mut retry = false;
                let mut save_example: Option<(u16, String)> = None;

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.label(
                        RichText::new("Response")
                            .size(12.5)
                            .family(theme::medium_family())
                            .color(p.text),
                    );
                    match &stored {
                        Some(s) => match &s.data {
                            Ok(data) => {
                                widgets::status_pill(ui, p, data.status);
                                let content_type = data
                                    .content_type
                                    .split(';')
                                    .next()
                                    .unwrap_or("")
                                    .trim()
                                    .to_string();
                                if !content_type.is_empty() {
                                    widgets::chip(
                                        ui,
                                        p,
                                        &content_type,
                                        p.muted,
                                        p.panel_alt,
                                        false,
                                    );
                                }
                                let timing = &data.timings;
                                let breakdown = format!(
                                    "Final URL: {}\n\nDNS: {}\nTCP connect: {}\nTLS handshake: {}\nTTFB: {}\nDownload: {}\nTotal: {}\n\nDNS, TCP and TLS come from a parallel probe connection.{}",
                                    data.final_url,
                                    timing
                                        .dns_ms
                                        .map(format_ms)
                                        .unwrap_or_else(|| "not reported".to_string()),
                                    timing
                                        .connect_ms
                                        .map(format_ms)
                                        .unwrap_or_else(|| "not reported".to_string()),
                                    timing
                                        .tls_ms
                                        .map(format_ms)
                                        .unwrap_or_else(|| "not reported".to_string()),
                                    format_ms(timing.ttfb_ms),
                                    format_ms(timing.download_ms),
                                    format_ms(timing.total_ms),
                                    if data.truncated {
                                        format!(
                                            "\nBody truncated at {}",
                                            format_bytes(data.size)
                                        )
                                    } else {
                                        String::new()
                                    }
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "TTFB {} · {} · {} · {}",
                                        format_ms(timing.ttfb_ms),
                                        format_ms(timing.total_ms),
                                        format_bytes(data.size),
                                        relative_time(s.at)
                                    ))
                                    .size(11.0)
                                    .color(p.faint),
                                )
                                .on_hover_text(breakdown);
                                if let (Some(dns), Some(connect), Some(tls)) = (
                                    timing.dns_ms,
                                    timing.connect_ms,
                                    timing.tls_ms,
                                ) {
                                    ui.label(
                                        RichText::new(format!(
                                            "· DNS {} · TCP {} · TLS {}",
                                            format_ms(dns),
                                            format_ms(connect),
                                            format_ms(tls)
                                        ))
                                        .size(10.0)
                                        .color(p.faint),
                                    );
                                } else if let (Some(dns), Some(connect)) =
                                    (timing.dns_ms, timing.connect_ms)
                                {
                                    ui.label(
                                        RichText::new(format!(
                                            "· DNS {} · TCP {}",
                                            format_ms(dns),
                                            format_ms(connect)
                                        ))
                                        .size(10.0)
                                        .color(p.faint),
                                    );
                                }
                                if data.truncated {
                                    widgets::chip(
                                        ui,
                                        p,
                                        "truncated",
                                        p.warn,
                                        p.warn.gamma_multiply(0.16),
                                        false,
                                    );
                                }
                            }
                            Err(_) => {
                                widgets::chip(
                                    ui,
                                    p,
                                    "Request failed",
                                    p.danger,
                                    p.danger_soft,
                                    true,
                                );
                            }
                        },
                        None => {
                            widgets::chip(ui, p, "EXAMPLE", p.muted, p.panel_alt, true)
                                .on_hover_text(
                                    "Sample response saved with the request. Send it to see live data.",
                                );
                            widgets::status_pill(ui, p, example_status);
                            ui.label(
                                RichText::new(format!(
                                    "sample payload · {}",
                                    format_bytes(example_body.len())
                                ))
                                .size(11.0)
                                .color(p.faint),
                            );
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if stored.is_some()
                            && widgets::icon_button(ui, p, Icon::Close, 24.0, "Clear response")
                                .clicked()
                        {
                            clear = true;
                        }
                        let body = response_body(&stored, &example_body);
                        if widgets::icon_button(ui, p, Icon::Copy, 24.0, "Copy body").clicked() {
                            ui.ctx().copy_text(body);
                            copy = true;
                        }
                        if matches!(&stored, Some(s) if s.data.is_err()) {
                            if widgets::icon_button(ui, p, Icon::Play, 24.0, "Retry").clicked() {
                                retry = true;
                            }
                        }
                        if let Some(data) =
                            stored.as_ref().and_then(|s| s.data.as_ref().ok())
                        {
                            if widgets::button(ui, p, "Save as example", Btn::Ghost)
                                .clicked()
                            {
                                save_example = Some((data.status, data.body.clone()));
                            }
                        }
                        let mut view_index = match self.resp_view {
                            RespView::Tree => 0,
                            RespView::Pretty => 1,
                            RespView::Raw => 2,
                        };
                        if widgets::segmented(
                            ui,
                            p,
                            &["Tree", "Pretty", "Raw"],
                            &mut view_index,
                        ) {
                            self.resp_view = match view_index {
                                0 => RespView::Tree,
                                1 => RespView::Pretty,
                                _ => RespView::Raw,
                            };
                        }
                    });
                });
                ui.add_space(4.0);

                let body_text = response_body(&stored, &example_body);
                if self.resp_view == RespView::Tree {
                    self.json_tree.prepare(&body_text);
                }

                let cookies = response_cookies(&stored);
                let headers_count = stored
                    .as_ref()
                    .and_then(|s| s.data.as_ref().ok())
                    .map(|d| d.headers.len())
                    .unwrap_or(0);
                let tests_count = stored
                    .as_ref()
                    .map(|s| s.assertions.len())
                    .unwrap_or(req.assertions.len());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    for (tab, label) in [
                        (RespTab::Body, "Body".to_string()),
                        (RespTab::Headers, format!("Headers{}", count_suffix(headers_count))),
                        (RespTab::Cookies, format!("Cookies{}", count_suffix(cookies.len()))),
                        (RespTab::Tests, format!("Tests{}", count_suffix(tests_count))),
                    ] {
                        if small_tab(ui, p, &label, self.resp_tab == tab) {
                            self.resp_tab = tab;
                        }
                    }
                    if self.resp_tab == RespTab::Body
                        && self.resp_view == RespView::Tree
                        && self.json_tree.is_json
                    {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if self.json_tree.is_truncated() {
                                ui.label(
                                    RichText::new("Preview truncated")
                                        .size(10.0)
                                        .color(p.warn),
                                );
                                ui.add_space(6.0);
                            }
                            json_tree::search_box(ui, p, &mut self.json_tree);
                            let query = self.json_tree.search().trim().to_string();
                            if !query.is_empty() {
                                let count = self.json_tree.matches;
                                let label = if count == 1 {
                                    "1 match".to_string()
                                } else {
                                    format!("{count} matches")
                                };
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(label).size(10.5).color(if count == 0 {
                                        p.warn
                                    } else {
                                        p.muted
                                    }),
                                );
                            }
                        });
                    }
                });
                let strip = ui.available_rect_before_wrap();
                ui.painter().hline(
                    strip.x_range(),
                    strip.top(),
                    Stroke::new(1.0_f32, p.border),
                );
                ui.add_space(6.0);

                match self.resp_tab {
                    RespTab::Body => {
                        let error = stored
                            .as_ref()
                            .and_then(|s| s.data.as_ref().err().cloned());
                        if let Some(error) = error {
                            Frame::NONE
                                .fill(p.danger_soft)
                                .stroke(Stroke::new(1.0_f32, p.danger.gamma_multiply(0.4)))
                                .corner_radius(CornerRadius::same(8))
                                .inner_margin(Margin::same(14))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let (r, _) = ui.allocate_exact_size(
                                            Vec2::splat(18.0),
                                            Sense::hover(),
                                        );
                                        widgets::paint_icon(
                                            ui.painter(),
                                            r,
                                            Icon::Alert,
                                            p.danger,
                                            1.5,
                                        );
                                        ui.label(
                                            RichText::new("Could not complete the request")
                                                .size(12.5)
                                                .family(theme::medium_family())
                                                .color(p.danger),
                                        );
                                    });
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(error)
                                            .size(11.5)
                                            .monospace()
                                            .color(p.text),
                                    );
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new(
                                            "Check the URL, the active environment variables and that the server is running.",
                                        )
                                        .size(11.0)
                                        .color(p.muted),
                                    );
                                });
                        } else {
                            if body_text.trim().is_empty() {
                                widgets::empty_state(
                                    ui,
                                    p,
                                    Icon::Sparkle,
                                    "No content",
                                    "The response has an empty body.",
                                    120.0,
                                );
                            } else {
                                match self.resp_view {
                                    RespView::Tree if self.json_tree.is_json => {
                                        if let Some(path) = self.json_tree.show(ui, p) {
                                            self.capture_dialog = Some(CaptureDialog {
                                                request_id: id,
                                                variable: suggest_variable(&path),
                                                path,
                                            });
                                        }
                                    }
                                    RespView::Raw => widgets::code_view(
                                        ui,
                                        p,
                                        &body_text,
                                        self.data.settings.line_numbers,
                                        false,
                                        &format!("response_body_{id}"),
                                    ),
                                    _ => widgets::code_view(
                                        ui,
                                        p,
                                        &body_text,
                                        self.data.settings.line_numbers,
                                        true,
                                        &format!("response_body_{id}"),
                                    ),
                                }
                            }
                        }
                    }
                    RespTab::Headers => {
                        let headers_data = stored
                            .as_ref()
                            .and_then(|s| s.data.as_ref().ok())
                            .map(|d| d.headers.clone());
                        if let Some(headers) = headers_data {
                            ui.add_space(2.0);
                            ScrollArea::vertical()
                                .id_salt(("resp_headers", id))
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    widgets::header_rows(ui, p, &headers);
                                });
                        } else {
                            widgets::empty_state(
                                ui,
                                p,
                                Icon::More,
                                "No headers yet",
                                "Send the request to inspect the response headers.",
                                120.0,
                            );
                        }
                    }
                    RespTab::Cookies => {
                        if cookies.is_empty() {
                            widgets::empty_state(
                                ui,
                                p,
                                Icon::Lock,
                                "No cookies",
                                "This response did not set any cookies.",
                                120.0,
                            );
                        } else {
                            ScrollArea::vertical()
                                .id_salt(("resp_cookies", id))
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    widgets::header_rows(ui, p, &cookies);
                                });
                        }
                    }
                    RespTab::Tests => {
                        ScrollArea::vertical()
                            .id_salt(("resp_tests", id))
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let results: Vec<(String, Option<bool>, String)> =
                                    if let Some(stored) = &stored {
                                        stored
                                            .assertions
                                            .iter()
                                            .map(|a| {
                                                (
                                                    a.label.clone(),
                                                    Some(a.passed),
                                                    a.detail.clone(),
                                                )
                                            })
                                            .collect()
                                    } else {
                                        req.assertions
                                            .iter()
                                            .filter(|a| a.enabled)
                                            .map(|a| {
                                                (a.label(), None, "Not run yet".to_string())
                                            })
                                            .collect()
                                    };
                                if results.is_empty() {
                                    widgets::empty_state(
                                        ui,
                                        p,
                                        Icon::Flask,
                                        "No assertions",
                                        "Add assertions in the request Tests tab to check responses automatically.",
                                        120.0,
                                    );
                                } else {
                                    for (label, state, detail) in results {
                                        assertion_result_row(ui, p, &label, state, &detail);
                                    }
                                }
                            });
                    }
                }

                if let Some((status, body)) = save_example {
                    if let Some(request) = self.data.find_request_mut(id) {
                        request.example_status = status;
                        request.example_body = body;
                    }
                    self.mock_dirty = true;
                    self.touch();
                    self.toast(ToastTone::Success, "Saved response as the request example");
                }
                if clear {
                    self.responses.remove(&id);
                }
                if copy {
                    self.toast(ToastTone::Neutral, "Response body copied");
                }
                if retry {
                    self.start_send(req.clone());
                }
            });
    }

    // -----------------------------------------------------------------------
    // Secondary views
    // -----------------------------------------------------------------------

    fn page_header(&self, ui: &mut Ui, p: &Palette, title: &str, subtitle: &str) {
        ui.add_space(18.0);
        ui.label(
            RichText::new(title)
                .family(theme::display_family())
                .size(24.0)
                .extra_letter_spacing(-0.3)
                .color(p.text),
        );
        ui.add_space(2.0);
        ui.label(RichText::new(subtitle).size(12.0).color(p.muted));
        ui.add_space(14.0);
    }

    fn history_view(&mut self, ui: &mut Ui, p: &Palette) {
        let mut open: Option<Id> = None;
        let mut clear = false;
        Frame::NONE
            .inner_margin(Margin::symmetric(22, 0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        self.page_header(
                            ui,
                            p,
                            "History",
                            "Every request sent from this workspace, newest first.",
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::button(ui, p, "Clear history", Btn::Ghost).clicked() {
                            clear = true;
                        }
                    });
                });
                let entries = self.data.history.clone();
                if entries.is_empty() {
                    widgets::empty_state(
                        ui,
                        p,
                        Icon::History,
                        "No requests sent yet",
                        "Send a request and it will show up here, with its status and timing.",
                        300.0,
                    );
                    return;
                }
                widgets::card_frame(p).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let width = ui.available_width();
                    widgets::table_header(
                        ui,
                        p,
                        &["Method", "URL", "Status", "Time", "Size", "When"],
                        &[70.0, width - 470.0, 90.0, 80.0, 80.0, 80.0],
                    );
                    for entry in entries {
                        let (rect, resp) =
                            ui.allocate_exact_size(vec2(width, 34.0), Sense::click());
                        if resp.hovered() {
                            ui.painter()
                                .rect_filled(rect, CornerRadius::same(6), p.hover);
                            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                        }
                        let badge = egui::Rect::from_min_size(
                            pos2(rect.left() + 4.0, rect.center().y - 9.0),
                            vec2(46.0, 18.0),
                        );
                        let color = p.method_color(entry.method);
                        ui.painter().rect_filled(
                            badge,
                            CornerRadius::same(4),
                            theme::mix(color, p.panel, 0.9),
                        );
                        ui.painter().text(
                            badge.center() + vec2(0.0, 0.5),
                            Align2::CENTER_CENTER,
                            entry.method.as_str(),
                            FontId::proportional(8.8),
                            color,
                        );
                        let url_font = FontId::monospace(11.5);
                        let url = elide(ui.painter(), &entry.url, &url_font, width - 500.0);
                        ui.painter().text(
                            pos2(rect.left() + 66.0, rect.center().y + 0.5),
                            Align2::LEFT_CENTER,
                            url,
                            url_font,
                            p.text,
                        );
                        match entry.status {
                            Some(status) => {
                                ui.painter().text(
                                    pos2(rect.left() + width - 396.0, rect.center().y + 0.5),
                                    Align2::LEFT_CENTER,
                                    status.to_string(),
                                    FontId::proportional(11.5),
                                    p.status_color(status),
                                );
                            }
                            None => {
                                ui.painter().text(
                                    pos2(rect.left() + width - 396.0, rect.center().y + 0.5),
                                    Align2::LEFT_CENTER,
                                    "failed",
                                    FontId::proportional(11.5),
                                    p.danger,
                                );
                            }
                        }
                        ui.painter().text(
                            pos2(rect.left() + width - 310.0, rect.center().y + 0.5),
                            Align2::LEFT_CENTER,
                            format_ms(entry.ms),
                            FontId::proportional(11.0),
                            p.muted,
                        );
                        ui.painter().text(
                            pos2(rect.left() + width - 230.0, rect.center().y + 0.5),
                            Align2::LEFT_CENTER,
                            format_bytes(entry.size as usize),
                            FontId::proportional(11.0),
                            p.muted,
                        );
                        ui.painter().text(
                            pos2(rect.right() - 12.0, rect.center().y + 0.5),
                            Align2::RIGHT_CENTER,
                            relative_time(entry.at),
                            FontId::proportional(10.5),
                            p.faint,
                        );
                        if resp.clicked() {
                            if let Some(request_id) = entry.request_id {
                                open = Some(request_id);
                            }
                        }
                    }
                });
            });
        if clear {
            self.data.history.clear();
            self.touch();
            self.toast(ToastTone::Neutral, "History cleared");
        }
        if let Some(id) = open {
            if self.data.find_request(id).is_some() {
                self.open_request(id);
            }
        }
    }

    fn environments_view(&mut self, ui: &mut Ui, p: &Palette) {
        let mut changed = false;
        let mut new_env = false;
        Frame::NONE
            .inner_margin(Margin::symmetric(22, 0))
            .show(ui, |ui| {
                self.page_header(
                    ui,
                    p,
                    "Environments",
                    "Reusable values for URLs, headers, bodies and auth. Reference them with {{name}}.",
                );
                ui.horizontal_top(|ui| {
                    let list_w = 230.0;
                    let total_w = ui.available_width();
                    ui.allocate_ui_with_layout(
                        vec2(list_w, ui.available_height()),
                        Layout::top_down(Align::Min),
                        |ui| {
                            widgets::card_frame(p).show(ui, |ui| {
                                ui.set_width(list_w - 34.0);
                                for env in &self.data.envs {
                                    let selected = self.env_selected == Some(env.id);
                                    let (rect, resp) = ui.allocate_exact_size(
                                        vec2(ui.available_width(), 34.0),
                                        Sense::click(),
                                    );
                                    if selected {
                                        ui.painter().rect_filled(
                                            rect,
                                            CornerRadius::same(6),
                                            p.selected,
                                        );
                                    } else if resp.hovered() {
                                        ui.painter().rect_filled(
                                            rect,
                                            CornerRadius::same(6),
                                            p.hover,
                                        );
                                    }
                                    let active = self.data.active_env == Some(env.id);
                                    if active {
                                        ui.painter().circle_filled(
                                            pos2(rect.left() + 10.0, rect.center().y),
                                            3.0,
                                            p.success,
                                        );
                                    }
                                    ui.painter().text(
                                        pos2(rect.left() + 20.0, rect.center().y + 0.5),
                                        Align2::LEFT_CENTER,
                                        &env.name,
                                        FontId::proportional(12.5),
                                        if selected { p.accent } else { p.text },
                                    );
                                    ui.painter().text(
                                        pos2(rect.right() - 10.0, rect.center().y + 0.5),
                                        Align2::RIGHT_CENTER,
                                        env.vars.len().to_string(),
                                        FontId::proportional(10.0),
                                        p.faint,
                                    );
                                    if resp.clicked() {
                                        self.env_selected = Some(env.id);
                                    }
                                }
                                ui.add_space(6.0);
                                if widgets::wide_button(ui, p, "+ New environment", Btn::Secondary)
                                    .clicked()
                                {
                                    new_env = true;
                                }
                            });
                        },
                    );
                    ui.add_space(14.0);
                    let editor_w = (total_w - list_w - 20.0).max(300.0);
                    ui.allocate_ui_with_layout(
                        vec2(editor_w, ui.available_height()),
                        Layout::top_down(Align::Min),
                        |ui| {
                            let selected = self
                                .env_selected
                                .or(self.data.active_env)
                                .and_then(|id| self.data.envs.iter().position(|e| e.id == id))
                                .unwrap_or(0);
                            let Some(mut env) = self.data.envs.get(selected).cloned() else {
                                return;
                            };
                            let env_id = env.id;
                            let active_env = self.data.active_env;
                            widgets::card_frame(p).show(ui, |ui| {
                                ui.set_width(editor_w - 34.0);
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Name").size(11.0).color(p.faint));
                                    let mut name = env.name.clone();
                                    if ui
                                        .add_sized(
                                            [240.0, 28.0],
                                            TextEdit::singleline(&mut name)
                                                .font(FontId::proportional(12.5)),
                                        )
                                        .changed()
                                    {
                                        env.name = name;
                                        changed = true;
                                    }
                                    ui.with_layout(
                                        Layout::right_to_left(Align::Center),
                                        |ui| {
                                            let is_active = active_env == Some(env_id);
                                            if is_active {
                                                widgets::chip(
                                                    ui,
                                                    p,
                                                    "Active",
                                                    p.success,
                                                    p.success_soft,
                                                    true,
                                                );
                                            } else if widgets::button(
                                                ui,
                                                p,
                                                "Set active",
                                                Btn::Secondary,
                                            )
                                            .clicked()
                                            {
                                                self.data.active_env = Some(env_id);
                                                self.touch();
                                            }
                                        },
                                    );
                                });
                                ui.add_space(10.0);
                                widgets::divider(ui, p);
                                ui.add_space(8.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("Variables")
                                            .size(11.5)
                                            .family(theme::medium_family())
                                            .color(p.text),
                                    );
                                    ui.label(
                                        RichText::new("secret values stay masked")
                                            .size(10.5)
                                            .color(p.faint),
                                    );
                                });
                                ui.add_space(6.0);
                                let mut remove: Option<usize> = None;
                                let width = ui.available_width();
                                for (i, var) in env.vars.iter_mut().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 6.0;
                                        let mut enabled = var.enabled;
                                        if ui
                                            .add(egui::Checkbox::without_text(&mut enabled))
                                            .changed()
                                        {
                                            var.enabled = enabled;
                                            changed = true;
                                        }
                                        if ui
                                            .add_sized(
                                                [width * 0.28, 26.0],
                                                TextEdit::singleline(&mut var.key)
                                                    .hint_text("Key")
                                                    .font(FontId::monospace(11.5))
                                                    .margin(Margin::symmetric(6, 4)),
                                            )
                                            .changed()
                                        {
                                            changed = true;
                                        }
                                        let revealed = self.revealed.contains(&(env_id, i));
                                        let value_edit =
                                            TextEdit::singleline(&mut var.value)
                                                .hint_text("Value")
                                                .font(FontId::monospace(11.5))
                                                .margin(Margin::symmetric(6, 4))
                                                .password(var.secret && !revealed);
                                        if ui
                                            .add_sized([width * 0.4, 26.0], value_edit)
                                            .changed()
                                        {
                                            changed = true;
                                        }
                                        let eye = if revealed {
                                            Icon::Sparkle
                                        } else {
                                            Icon::Lock
                                        };
                                        if widgets::icon_button(
                                            ui,
                                            p,
                                            eye,
                                            22.0,
                                            "Show or hide value",
                                        )
                                        .clicked()
                                        {
                                            if revealed {
                                                self.revealed.remove(&(env_id, i));
                                            } else {
                                                self.revealed.insert((env_id, i));
                                            }
                                        }
                                        let mut secret = var.secret;
                                        if ui
                                            .add(egui::Checkbox::without_text(&mut secret))
                                            .on_hover_text("Keep this value masked")
                                            .changed()
                                        {
                                            var.secret = secret;
                                            changed = true;
                                        }
                                        ui.label(
                                            RichText::new("secret")
                                                .size(10.0)
                                                .color(p.faint),
                                        );
                                        if widgets::icon_button(
                                            ui,
                                            p,
                                            Icon::Trash,
                                            22.0,
                                            "Remove",
                                        )
                                        .clicked()
                                        {
                                            remove = Some(i);
                                        }
                                    });
                                }
                                if let Some(i) = remove {
                                    env.vars.remove(i);
                                    changed = true;
                                }
                                ui.add_space(4.0);
                                if widgets::button(ui, p, "+ Add variable", Btn::Ghost).clicked() {
                                    env.vars.push(KV::new());
                                    changed = true;
                                }
                            });
                            if let Some(slot) = self.data.envs.get_mut(selected) {
                                *slot = env;
                            }
                        },
                    );
                });
            });
        if new_env {
            let env = crate::model::Environment::new("New environment");
            self.env_selected = Some(env.id);
            self.data.envs.push(env);
            self.touch();
            self.toast(ToastTone::Neutral, "Environment created");
        }
        if changed {
            self.touch();
        }
    }

    fn tests_view(&mut self, ui: &mut Ui, p: &Palette) {
        let mut run = false;
        Frame::NONE
            .inner_margin(Margin::symmetric(22, 0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        self.page_header(
                            ui,
                            p,
                            "Tests",
                            "Assertions across every request in this workspace.",
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let label = if self.test_running {
                            "Running…"
                        } else {
                            "Run all tests"
                        };
                        if widgets::button(ui, p, label, Btn::Primary).clicked()
                            && !self.test_running
                        {
                            run = true;
                        }
                    });
                });

                let total_assertions: usize = self
                    .data
                    .all_requests()
                    .iter()
                    .map(|r| r.assertions.iter().filter(|a| a.enabled).count())
                    .sum();
                let passed: usize = self.test_results.iter().map(|r| r.passed()).sum();
                let failed: usize = self.test_results.iter().map(|r| r.failed()).sum();

                ui.horizontal(|ui| {
                    for (label, value, tone) in [
                        ("Requests", self.data.total_requests().to_string(), p.text),
                        ("Assertions", total_assertions.to_string(), p.text),
                        ("Passed", passed.to_string(), p.success),
                        ("Failed", failed.to_string(), p.danger),
                        (
                            "Duration",
                            if self.test_duration_ms == 0 {
                                "—".to_string()
                            } else {
                                format_ms(self.test_duration_ms)
                            },
                            p.text,
                        ),
                    ] {
                        widgets::card_frame(p).show(ui, |ui| {
                            ui.set_width(142.0);
                            ui.label(RichText::new(label).size(10.5).color(p.faint));
                            ui.label(RichText::new(value).size(19.0).family(theme::display_family()).color(tone));
                        });
                    }
                });
                ui.add_space(14.0);

                if self.test_running {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new("Sending requests and checking assertions…")
                                .size(12.0)
                                .color(p.muted),
                        );
                    });
                }

                if self.test_results.is_empty() && !self.test_running {
                    widgets::empty_state(
                        ui,
                        p,
                        Icon::Flask,
                        "No test run yet",
                        "Run all tests to send every request and evaluate its assertions.",
                        260.0,
                    );
                    return;
                }

                let results = self.test_results.clone();
                ScrollArea::vertical()
                    .id_salt("test_results")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for outcome in &results {
                            let ok = outcome.failed() == 0
                                && outcome
                                    .data
                                    .as_ref()
                                    .map(|d| d.status < 400)
                                    .unwrap_or(false);
                            widgets::card_frame(p).show(ui, |ui| {
                                ui.set_width(ui.available_width() - 34.0);
                                ui.horizontal(|ui| {
                                    widgets::method_badge(ui, p, outcome.method);
                                    ui.label(
                                        RichText::new(&outcome.request_name)
                                            .size(12.5)
                                            .family(theme::medium_family())
                                            .color(p.text),
                                    );
                                    widgets::chip(
                                        ui,
                                        p,
                                        &outcome.folder,
                                        p.muted,
                                        p.panel_alt,
                                        false,
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if let Some(status) = outcome.status() {
                                            widgets::status_pill(ui, p, status);
                                        }
                                        ui.label(
                                            RichText::new(format!(
                                                "{}/{} passed",
                                                outcome.passed(),
                                                outcome.assertions.len()
                                            ))
                                            .size(11.0)
                                            .color(if ok { p.success } else { p.danger }),
                                        );
                                    });
                                });
                                ui.add_space(4.0);
                                for assertion in &outcome.assertions {
                                    assertion_result_row(
                                        ui,
                                        p,
                                        &assertion.label,
                                        Some(assertion.passed),
                                        &assertion.detail,
                                    );
                                }
                                if let Err(err) = &outcome.data {
                                    ui.label(
                                        RichText::new(err).size(11.0).color(p.danger).monospace(),
                                    );
                                }
                            });
                            ui.add_space(8.0);
                        }
                    });
            });
        if run {
            self.start_test_run();
        }
    }

    fn mock_view(&mut self, ui: &mut Ui, p: &Palette) {
        let mut toggle = false;
        let mut port_changed = false;
        let mut copy_url = false;
        Frame::NONE
            .inner_margin(Margin::symmetric(22, 0))
            .show(ui, |ui| {
                self.page_header(
                    ui,
                    p,
                    "Mock server",
                    "Serve sample responses for the routes you marked as mocks.",
                );
                widgets::card_frame(p).show(ui, |ui| {
                    ui.set_width(ui.available_width() - 34.0);
                    ui.horizontal(|ui| {
                        let running = self.mock.running;
                        let mut on = running;
                        widgets::toggle_switch(ui, p, &mut on);
                        if on != running {
                            toggle = true;
                        }
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(if running {
                                "Server running"
                            } else {
                                "Server stopped"
                            })
                            .size(13.0)
                            .family(theme::medium_family())
                            .color(if running { p.success } else { p.text }),
                        );
                        if running {
                            widgets::chip(ui, p, "live", p.success, p.success_soft, true);
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let mut port = self.data.settings.mock_port as i32;
                            if ui
                                .add_sized(
                                    [64.0, 26.0],
                                    egui::DragValue::new(&mut port)
                                        .range(1024..=65535)
                                        .speed(1.0),
                                )
                                .changed()
                            {
                                self.data.settings.mock_port = port as u16;
                                port_changed = true;
                            }
                            ui.add_space(2.0);
                            ui.label(RichText::new("Port").size(11.0).color(p.faint));
                        });
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(self.mock.base_url())
                                .size(12.0)
                                .monospace()
                                .color(p.muted),
                        );
                        if widgets::icon_button(ui, p, Icon::Copy, 22.0, "Copy base URL")
                            .clicked()
                        {
                            copy_url = true;
                        }
                    });
                    if let Some(err) = &self.mock.error {
                        ui.add_space(4.0);
                        ui.label(RichText::new(err).size(11.0).color(p.danger));
                    }
                });
                ui.add_space(14.0);

                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Routes")
                            .size(13.0)
                            .family(theme::medium_family())
                            .color(p.text),
                    );
                    ui.label(
                        RichText::new("toggle “Mock this route” from a request menu")
                            .size(11.0)
                            .color(p.faint),
                    );
                });
                ui.add_space(6.0);

                let mut entries: Vec<(usize, usize)> = Vec::new();
                for (folder_index, folder) in self.data.folders.iter().enumerate() {
                    for (request_index, request) in folder.requests.iter().enumerate() {
                        if request.mock {
                            entries.push((folder_index, request_index));
                        }
                    }
                }
                if entries.is_empty() {
                    widgets::empty_state(
                        ui,
                        p,
                        Icon::Server,
                        "No mock routes",
                        "Mark a request as a mock route and its example response will be served here.",
                        200.0,
                    );
                } else {
                    let hits = self.mock.hits();
                    let mut simulated = false;
                    widgets::card_frame(p).show(ui, |ui| {
                        ui.set_width(ui.available_width() - 34.0);
                        let width = ui.available_width();
                        widgets::table_header(
                            ui,
                            p,
                            &[
                                "Method",
                                "Path",
                                "Status",
                                "Delay",
                                "Behaviour",
                                "Hits",
                            ],
                            &[64.0, width - 470.0, 60.0, 100.0, 150.0, 50.0],
                        );
                        for (folder_index, request_index) in &entries {
                            let (method, path, status, latency, failure) = {
                                let request =
                                    &self.data.folders[*folder_index].requests[*request_index];
                                let path = reqwest::Url::parse(&crate::http::resolved_url(
                                    request,
                                    &self.data.env_map(),
                                ))
                                .map(|url| url.path().to_string())
                                .unwrap_or_else(|_| request.url.clone());
                                (
                                    request.method,
                                    path,
                                    if request.example_status == 0 {
                                        200
                                    } else {
                                        request.example_status
                                    },
                                    request.mock_latency_ms,
                                    request.mock_failure,
                                )
                            };
                            let hit_count = hits
                                .iter()
                                .filter(|hit| {
                                    hit.path == path
                                        && hit.method.eq_ignore_ascii_case(method.as_str())
                                })
                                .count();
                            let (rect, _) =
                                ui.allocate_exact_size(vec2(width, 34.0), Sense::hover());
                            let badge = egui::Rect::from_min_size(
                                pos2(rect.left() + 4.0, rect.center().y - 9.0),
                                vec2(46.0, 18.0),
                            );
                            let color = p.method_color(method);
                            ui.painter().rect_filled(
                                badge,
                                CornerRadius::same(4),
                                theme::mix(color, p.panel, 0.9),
                            );
                            ui.painter().text(
                                badge.center() + vec2(0.0, 0.5),
                                Align2::CENTER_CENTER,
                                method.as_str(),
                                FontId::proportional(8.8),
                                color,
                            );
                            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(
                                egui::Rect::from_min_size(
                                    pos2(rect.left() + 62.0, rect.top() + 4.0),
                                    vec2(width - 66.0, 26.0),
                                ),
                            ));
                            child.horizontal(|ui| {
                                ui.add_sized(
                                    [width - 470.0, 24.0],
                                    egui::Label::new(
                                        RichText::new(&path)
                                            .size(11.5)
                                            .monospace()
                                            .color(p.text),
                                    )
                                    .truncate(),
                                );
                                ui.add_sized(
                                    [60.0, 24.0],
                                    egui::Label::new(
                                        RichText::new(status.to_string())
                                            .size(11.5)
                                            .color(p.status_color(status)),
                                    ),
                                );
                                let mut delay = latency as i32;
                                if ui
                                    .add_sized(
                                        [100.0, 24.0],
                                        egui::DragValue::new(&mut delay)
                                            .range(0..=30_000)
                                            .speed(25.0)
                                            .suffix(" ms"),
                                    )
                                    .changed()
                                {
                                    if let Some(request) = self
                                        .data
                                        .folders
                                        .get_mut(*folder_index)
                                        .and_then(|folder| folder.requests.get_mut(*request_index))
                                    {
                                        request.mock_latency_ms = delay.max(0) as u64;
                                    }
                                    simulated = true;
                                }
                                let mut index = failure.index();
                                egui::ComboBox::from_id_salt((
                                    "mock_behaviour",
                                    folder_index,
                                    request_index,
                                ))
                                .width(150.0)
                                .selected_text(RichText::new(failure.label()).size(11.0))
                                .show_ui(ui, |ui| {
                                    for (position, option) in
                                        crate::model::MockFailure::ALL.iter().enumerate()
                                    {
                                        if ui
                                            .selectable_label(index == position, option.label())
                                            .clicked()
                                        {
                                            index = position;
                                        }
                                    }
                                });
                                if index != failure.index() {
                                    if let Some(request) = self
                                        .data
                                        .folders
                                        .get_mut(*folder_index)
                                        .and_then(|folder| folder.requests.get_mut(*request_index))
                                    {
                                        request.mock_failure =
                                            crate::model::MockFailure::from_index(index);
                                    }
                                    simulated = true;
                                }
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(
                                        RichText::new(hit_count.to_string())
                                            .size(11.0)
                                            .color(if hit_count > 0 { p.muted } else { p.faint }),
                                    );
                                });
                            });
                        }
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(
                                "Override per request with X-Mock-Status, X-Mock-Delay or X-Mock-Fail: timeout|reset.",
                            )
                            .size(10.5)
                            .color(p.faint),
                        );
                    });
                    if simulated {
                        self.mock_dirty = true;
                        self.touch();
                    }
                }

                ui.add_space(14.0);
                ui.label(
                    RichText::new("Recent hits")
                        .size(13.0)
                        .family(theme::medium_family())
                        .color(p.text),
                );
                ui.add_space(4.0);
                let hits = self.mock.hits();
                if hits.is_empty() {
                    ui.label(
                        RichText::new(if self.mock.running {
                            "Waiting for requests…"
                        } else {
                            "Start the server to see requests arrive."
                        })
                        .size(11.5)
                        .color(p.faint),
                    );
                } else {
                    widgets::card_frame(p).show(ui, |ui| {
                        ui.set_width(ui.available_width() - 34.0);
                        for hit in hits.iter().take(10) {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(&hit.method)
                                        .size(10.5)
                                        .family(theme::medium_family())
                                        .monospace()
                                        .color(p.muted),
                                );
                                ui.label(
                                    RichText::new(&hit.path)
                                        .size(11.5)
                                        .monospace()
                                        .color(p.text),
                                );
                                ui.with_layout(
                                    Layout::right_to_left(Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new(relative_time(hit.at))
                                                .size(10.0)
                                                .color(p.faint),
                                        );
                                        ui.label(
                                            RichText::new(hit.status.to_string())
                                                .size(11.0)
                                                .color(p.status_color(hit.status)),
                                        );
                                    },
                                );
                            });
                        }
                    });
                }
            });

        if toggle {
            if self.mock.running {
                self.mock.stop();
                self.toast(ToastTone::Neutral, "Mock server stopped");
            } else {
                let port = self.data.settings.mock_port;
                let routes = self.mock_routes();
                self.mock.start(port, routes);
                self.mock_dirty = false;
                if self.mock.running {
                    self.toast(
                        ToastTone::Success,
                        format!("Mock server listening on {}", self.mock.base_url()),
                    );
                } else {
                    self.toast(ToastTone::Danger, "The mock server could not start");
                }
            }
        }
        if port_changed {
            self.touch();
            if self.mock.running {
                let port = self.data.settings.mock_port;
                let routes = self.mock_routes();
                self.mock.start(port, routes);
                self.toast(
                    ToastTone::Neutral,
                    format!("Mock server moved to port {port}"),
                );
            }
        }
        if copy_url {
            let url = self.mock.base_url();
            ui.ctx().copy_text(url);
            self.toast(ToastTone::Neutral, "Base URL copied");
        }
    }

    fn docs_view(&mut self, ui: &mut Ui, p: &Palette) {
        let anchors = docs::anchors(&self.data);
        let mut publish_clicked = false;
        let mut copy_link: Option<String> = None;
        let base = self
            .data
            .settings
            .docs_base_url
            .trim()
            .trim_end_matches('/')
            .to_string();
        let base_ref = &base;
        Frame::NONE
            .inner_margin(Margin::symmetric(22, 0))
            .show(ui, |ui| {
                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&self.data.name)
                            .family(theme::display_family())
                            .size(24.0)
                            .extra_letter_spacing(-0.3)
                            .color(p.text),
                    );
                    widgets::chip(ui, p, "v1.0", p.muted, p.panel_alt, false);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::button(ui, p, "Publish…", Btn::Primary).clicked() {
                            publish_clicked = true;
                        }
                        if !self.data.settings.docs_base_url.trim().is_empty() {
                            widgets::chip(
                                ui,
                                p,
                                "public URL set",
                                p.success,
                                p.success_soft,
                                false,
                            );
                        }
                    });
                });
                ui.add_space(2.0);
                ui.label(
                    RichText::new(
                        "Generated reference for every request. Publish it as a read-only site and share the links.",
                    )
                    .size(12.0)
                    .color(p.muted),
                );
                ui.add_space(12.0);
                let base = self
                    .data
                    .env_map()
                    .get("baseUrl")
                    .cloned()
                    .unwrap_or_else(|| "https://api.example.com/v1".to_string());
                widgets::card_frame(p).show(ui, |ui| {
                    ui.set_width(ui.available_width() - 34.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Base URL").size(11.0).color(p.faint));
                        ui.label(RichText::new(base).size(12.0).monospace().color(p.text));
                    });
                });
                ui.add_space(10.0);
                ScrollArea::vertical()
                    .id_salt("docs_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for folder in &self.data.folders {
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new(&folder.name)
                                    .size(14.0)
                                    .family(theme::medium_family())
                                    .color(p.text),
                            );
                            ui.add_space(4.0);
                            for req in &folder.requests {
                                widgets::card_frame(p).show(ui, |ui| {
                                    ui.set_width(ui.available_width() - 34.0);
                                    ui.horizontal(|ui| {
                                        widgets::method_badge(ui, p, req.method);
                                        ui.label(
                                            RichText::new(&req.name)
                                                .size(13.0)
                                                .family(theme::medium_family())
                                                .color(p.text),
                                        );
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                if widgets::icon_button(
                                                    ui,
                                                    p,
                                                    Icon::Copy,
                                                    22.0,
                                                    "Copy share link",
                                                )
                                                .clicked()
                                                {
                                                    if let Some((_, anchor)) = anchors
                                                        .iter()
                                                        .find(|(id, _)| *id == req.id)
                                                    {
                                                        copy_link = Some(if base_ref.is_empty() {
                                                            format!("index.html#{anchor}")
                                                        } else {
                                                            format!("{base_ref}/index.html#{anchor}")
                                                        });
                                                    }
                                                }
                                                ui.label(
                                                    RichText::new(&req.url)
                                                        .size(11.5)
                                                        .monospace()
                                                        .color(p.muted),
                                                );
                                            },
                                        );
                                    });
                                    if !req.description.is_empty() {
                                        ui.add_space(2.0);
                                        ui.label(
                                            RichText::new(&req.description)
                                                .size(11.5)
                                                .color(p.muted),
                                        );
                                    }
                                    if !req.params.is_empty() {
                                        ui.add_space(8.0);
                                        widgets::section_label(ui, p, "QUERY PARAMETERS");
                                        ui.add_space(2.0);
                                        for kv in req.params.iter().filter(|k| k.enabled) {
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    RichText::new(&kv.key)
                                                        .size(11.0)
                                                        .monospace()
                                                        .color(p.text),
                                                );
                                                ui.label(
                                                    RichText::new(if kv.desc.is_empty() {
                                                        "—"
                                                    } else {
                                                        kv.desc.as_str()
                                                    })
                                                    .size(11.0)
                                                    .color(p.faint),
                                                );
                                            });
                                        }
                                    }
                                    if !req.example_body.trim().is_empty() {
                                        ui.add_space(8.0);
                                        ui.horizontal(|ui| {
                                            widgets::section_label(ui, p, "EXAMPLE RESPONSE");
                                            if req.example_status != 0 {
                                                widgets::status_pill(ui, p, req.example_status);
                                            }
                                        });
                                        ui.add_space(2.0);
                                        let code = widgets::pretty_json(&req.example_body)
                                            .unwrap_or_else(|| req.example_body.clone());
                                        Frame::NONE
                                            .fill(p.code_bg)
                                            .stroke(Stroke::new(1.0_f32, p.border))
                                            .corner_radius(CornerRadius::same(7))
                                            .inner_margin(Margin::same(10))
                                            .show(ui, |ui| {
                                                ui.set_min_width(ui.available_width() - 4.0);
                                                ui.label(
                                                    RichText::new(code)
                                                        .size(11.0)
                                                        .monospace()
                                                        .color(p.muted),
                                                );
                                            });
                                    }
                                });
                                ui.add_space(8.0);
                            }
                        }
                        ui.add_space(20.0);
                    });
            });
        if publish_clicked {
            self.open_publish();
        }
        if let Some(link) = copy_link {
            self.pending_copy = Some(link);
            self.toast(ToastTone::Success, "Share link copied");
        }
    }

    fn settings_view(&mut self, ui: &mut Ui, p: &Palette) {
        let mut changed = false;
        Frame::NONE
            .inner_margin(Margin::symmetric(22, 0))
            .show(ui, |ui| {
                self.page_header(
                    ui,
                    p,
                    "Settings",
                    "Workspace preferences. Everything is stored locally on this computer.",
                );
                ScrollArea::vertical()
                    .id_salt("settings_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        widgets::card_frame(p).show(ui, |ui| {
                            ui.set_width(ui.available_width() - 34.0);
                            ui.label(
                                RichText::new("Appearance")
                                    .size(13.0)
                                    .family(theme::medium_family())
                                    .color(p.text),
                            );
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Theme").size(12.0).color(p.muted));
                                let mut index = if self.data.settings.dark { 1 } else { 0 };
                                if widgets::segmented(ui, p, &["Light", "Dark"], &mut index) {
                                    self.data.settings.dark = index == 1;
                                    let palette = self.palette();
                                    theme::apply_style(ui.ctx(), &palette);
                                    changed = true;
                                }
                            });
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Show line numbers in responses")
                                        .size(12.0)
                                        .color(p.muted),
                                );
                                let mut on = self.data.settings.line_numbers;
                                widgets::toggle_switch(ui, p, &mut on);
                                if on != self.data.settings.line_numbers {
                                    self.data.settings.line_numbers = on;
                                    changed = true;
                                }
                            });
                        });
                        ui.add_space(12.0);
                        widgets::card_frame(p).show(ui, |ui| {
                            ui.set_width(ui.available_width() - 34.0);
                            ui.label(
                                RichText::new("Requests")
                                    .size(13.0)
                                    .family(theme::medium_family())
                                    .color(p.text),
                            );
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Timeout").size(12.0).color(p.muted));
                                let mut timeout = self.data.settings.timeout_ms as i32;
                                if ui
                                    .add_sized(
                                        [92.0, 26.0],
                                        egui::DragValue::new(&mut timeout)
                                            .range(500..=120_000)
                                            .speed(100.0)
                                            .suffix(" ms"),
                                    )
                                    .changed()
                                {
                                    self.data.settings.timeout_ms = timeout as u64;
                                    changed = true;
                                }
                                ui.add_space(12.0);
                                ui.label(
                                    RichText::new("Follow redirects")
                                        .size(12.0)
                                        .color(p.muted),
                                );
                                let mut on = self.data.settings.follow_redirects;
                                widgets::toggle_switch(ui, p, &mut on);
                                if on != self.data.settings.follow_redirects {
                                    self.data.settings.follow_redirects = on;
                                    changed = true;
                                }
                            });
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Proxy").size(12.0).color(p.muted));
                                let mut proxy = self.data.settings.proxy.clone();
                                if ui
                                    .add_sized(
                                        [330.0, 26.0],
                                        TextEdit::singleline(&mut proxy)
                                            .hint_text("http://127.0.0.1:8080")
                                            .font(FontId::monospace(11.0))
                                            .margin(Margin::symmetric(6, 4)),
                                    )
                                    .changed()
                                {
                                    self.data.settings.proxy = proxy;
                                    changed = true;
                                }
                                ui.label(
                                    RichText::new("applies to requests and OAuth")
                                        .size(10.0)
                                        .color(p.faint),
                                );
                            });
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("CA bundle").size(12.0).color(p.muted),
                                );
                                let mut ca = self.data.settings.ca_bundle.clone();
                                if ui
                                    .add_sized(
                                        [330.0, 26.0],
                                        TextEdit::singleline(&mut ca)
                                            .hint_text("/path/to/ca.pem")
                                            .font(FontId::monospace(11.0))
                                            .margin(Margin::symmetric(6, 4)),
                                    )
                                    .changed()
                                {
                                    self.data.settings.ca_bundle = ca;
                                    changed = true;
                                }
                                if widgets::button(ui, p, "Browse…", Btn::Secondary)
                                    .clicked()
                                {
                                    if let Some(path) =
                                        rfd::FileDialog::new().pick_file()
                                    {
                                        self.data.settings.ca_bundle =
                                            path.display().to_string();
                                        changed = true;
                                    }
                                }
                            });
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Accept invalid certificates")
                                        .size(12.0)
                                        .color(p.muted),
                                );
                                let mut accept = self.data.settings.accept_invalid_certs;
                                widgets::toggle_switch(ui, p, &mut accept);
                                if accept != self.data.settings.accept_invalid_certs {
                                    self.data.settings.accept_invalid_certs = accept;
                                    changed = true;
                                }
                                if self.data.settings.accept_invalid_certs {
                                    widgets::chip(
                                        ui,
                                        p,
                                        "insecure",
                                        p.warn,
                                        p.warn.gamma_multiply(0.16),
                                        false,
                                    );
                                }
                            });
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Measure DNS / TCP / TLS")
                                        .size(12.0)
                                        .color(p.muted),
                                );
                                let mut measure = self.data.settings.measure_phases;
                                widgets::toggle_switch(ui, p, &mut measure);
                                if measure != self.data.settings.measure_phases {
                                    self.data.settings.measure_phases = measure;
                                    changed = true;
                                }
                                ui.label(
                                    RichText::new(
                                        "opens a parallel probe connection per request",
                                    )
                                    .size(10.0)
                                    .color(p.faint),
                                );
                            });
                        });
                        ui.add_space(12.0);
                        widgets::card_frame(p).show(ui, |ui| {
                            ui.set_width(ui.available_width() - 34.0);
                            ui.label(
                                RichText::new("Secrets")
                                    .size(13.0)
                                    .family(theme::medium_family())
                                    .color(p.text),
                            );
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                let mut on = self.data.settings.keychain_secrets;
                                widgets::toggle_switch(ui, p, &mut on);
                                if on != self.data.settings.keychain_secrets {
                                    self.data.settings.keychain_secrets = on;
                                    changed = true;
                                }
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new("Keep secrets in the macOS Keychain")
                                        .size(12.0)
                                        .color(p.muted),
                                );
                                if !crate::vault::available() {
                                    widgets::chip(
                                        ui,
                                        p,
                                        "macOS only",
                                        p.warn,
                                        p.warn.gamma_multiply(0.16),
                                        false,
                                    );
                                }
                            });
                            ui.add_space(6.0);
                            let slots = crate::model::secret_slot_count(&self.data);
                            ui.label(
                                RichText::new(format!(
                                    "{} secret value{} live outside the file as keychain:// references. \
                                     The workspace can be shared or committed without leaking them.",
                                    slots,
                                    if slots == 1 { "" } else { "s" }
                                ))
                                .size(11.0)
                                .color(p.faint),
                            );
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                if widgets::button(ui, p, "Check Keychain access", Btn::Secondary)
                                    .clicked()
                                {
                                    match crate::vault::self_test() {
                                        Ok(()) => self.toast(
                                            ToastTone::Success,
                                            "The Keychain accepts and returns secrets",
                                        ),
                                        Err(error) => self.toast(
                                            ToastTone::Danger,
                                            format!("Keychain check failed: {error}"),
                                        ),
                                    }
                                }
                                if widgets::button(ui, p, "Forget stored secrets", Btn::Ghost)
                                    .clicked()
                                {
                                    let report = crate::model::forget_secrets(&mut self.data);
                                    self.settings_flash = Some((
                                        Instant::now(),
                                        format!(
                                            "Removed {} item(s) from the Keychain. Values stay in memory until you save again.",
                                            report.sealed
                                        ),
                                    ));
                                }
                            });
                            if let Some((at, text)) = &self.settings_flash {
                                if at.elapsed().as_secs() < 8 {
                                    ui.add_space(4.0);
                                    ui.label(RichText::new(text).size(10.5).color(p.muted));
                                }
                            }
                        });
                        ui.add_space(12.0);
                        widgets::card_frame(p).show(ui, |ui| {
                            ui.set_width(ui.available_width() - 34.0);
                            ui.label(
                                RichText::new("Workspace")
                                    .size(13.0)
                                    .family(theme::medium_family())
                                    .color(p.text),
                            );
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(crate::model::storage_path().display().to_string())
                                    .size(11.5)
                                    .monospace()
                                    .color(p.muted),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(format!(
                                    "{} collections · {} requests · {} environments",
                                    self.data.folders.len(),
                                    self.data.total_requests(),
                                    self.data.envs.len()
                                ))
                                .size(11.0)
                                .color(p.faint),
                            );
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                if widgets::button(ui, p, "Save now", Btn::Secondary).clicked() {
                                    self.save_now();
                                    self.toast(ToastTone::Success, "Workspace saved");
                                }
                                if widgets::button(ui, p, "Reset demo data", Btn::DangerGhost)
                                    .clicked()
                                {
                                    self.confirm_reset = true;
                                }
                            });
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new(
                                    "Run this workspace in CI with `lucapi run --format junit --output report.xml`.",
                                )
                                .size(10.5)
                                .monospace()
                                .color(p.faint),
                            );
                        });
                        ui.add_space(12.0);
                        widgets::card_frame(p).show(ui, |ui| {
                            ui.set_width(ui.available_width() - 34.0);
                            ui.label(
                                RichText::new("About LucAPI")
                                    .size(13.0)
                                    .family(theme::medium_family())
                                    .color(p.text),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(
                                    "A local-first API design and testing workspace, rebuilt from a description of Apidog. Built with Rust and egui.",
                                )
                                .size(11.5)
                                .color(p.muted),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                                    .size(10.5)
                                    .color(p.faint),
                            );
                        });
                        ui.add_space(24.0);
                    });
            });
        if changed {
            self.touch_ui();
        }
    }

    fn status_bar(&self, ui: &mut Ui, p: &Palette) {
        ui.horizontal_centered(|ui| {
            ui.add_space(12.0);
            let (busy, text) = if self.sending.is_some() {
                (true, "Sending request…".to_string())
            } else if self.test_running {
                (true, "Running tests…".to_string())
            } else {
                (false, "Ready".to_string())
            };
            let (dot, _) = ui.allocate_exact_size(Vec2::splat(9.0), Sense::hover());
            ui.painter()
                .circle_filled(dot.center(), 3.0, if busy { p.warn } else { p.success });
            ui.label(RichText::new(text).size(10.5).color(p.muted));
            if let Some(env) = self.data.active_env() {
                ui.label(RichText::new("·").size(10.5).color(p.faint));
                ui.label(RichText::new(&env.name).size(10.5).color(p.muted));
            }
            if let Some(id) = self.active_tab {
                if let Some(request) = self.data.find_request(id) {
                    let missing = crate::vars::request_undefined(request, &self.defined_vars());
                    if !missing.is_empty() {
                        ui.label(RichText::new("·").size(10.5).color(p.faint));
                        let text = if missing.len() == 1 {
                            "1 undefined variable".to_string()
                        } else {
                            format!("{} undefined variables", missing.len())
                        };
                        ui.label(RichText::new(text).size(10.5).color(p.warn))
                            .on_hover_text(format!(
                                "Not defined in the active environment: {}",
                                missing.join(", ")
                            ));
                    }
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(12.0);
                ui.label(RichText::new(format!("LucAPI {}", env!("CARGO_PKG_VERSION"))).size(10.0).color(p.faint));
                ui.label(RichText::new("·").size(10.0).color(p.faint));
                let saved = if self.dirty {
                    "Unsaved changes".to_string()
                } else {
                    "Saved".to_string()
                };
                ui.label(RichText::new(saved).size(10.0).color(p.faint));
            });
        });
        let rect = ui.max_rect();
        ui.painter().hline(
            rect.x_range(),
            rect.top() + 0.5,
            Stroke::new(1.0_f32, p.border),
        );
    }

    // -----------------------------------------------------------------------
    // Overlays
    // -----------------------------------------------------------------------

    fn toast_ui(&mut self, ctx: &egui::Context, p: &Palette) {
        let Some(toast) = &self.toast else { return };
        if toast.at.elapsed() > Duration::from_secs(3) {
            self.toast = None;
            return;
        }
        let (bg, fg) = match toast.tone {
            ToastTone::Neutral => (p.text, p.panel),
            ToastTone::Success => (
                p.success,
                if p.dark {
                    Color32::from_rgb(0x08, 0x14, 0x0F)
                } else {
                    Color32::WHITE
                },
            ),
            ToastTone::Danger => (p.danger, Color32::WHITE),
        };
        let text = toast.text.clone();
        egui::Area::new("lucapi_toast".into())
            .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -46.0))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                Frame::NONE
                    .fill(bg)
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(16, 9))
                    .show(ui, |ui| {
                        ui.label(RichText::new(text).size(12.0).color(fg));
                    });
            });
        ctx.request_repaint_after(Duration::from_millis(200));
    }

    fn rename_window(&mut self, ctx: &egui::Context, p: &Palette) {
        let Some(mut state) = self.rename.take() else {
            return;
        };
        let mut close = false;
        let mut commit = false;
        let title = match state.target {
            RenameTarget::Project => "Rename workspace",
            RenameTarget::Folder(_) => "Rename collection",
            RenameTarget::Request(_) => "Rename request",
        };
        egui::Window::new(title)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .collapsible(false)
            .resizable(false)
            .min_width(330.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                let resp = ui.add_sized(
                    [ui.available_width(), 30.0],
                    TextEdit::singleline(&mut state.text).font(FontId::proportional(13.0)),
                );
                resp.request_focus();
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::button(ui, p, "Save", Btn::Primary).clicked()
                            || ui.input(|i| i.key_pressed(egui::Key::Enter))
                        {
                            commit = true;
                        }
                        if widgets::button(ui, p, "Cancel", Btn::Secondary).clicked()
                            || ui.input(|i| i.key_pressed(egui::Key::Escape))
                        {
                            close = true;
                        }
                    });
                });
            });
        if commit {
            let name = state.text.trim().to_string();
            if !name.is_empty() {
                match state.target {
                    RenameTarget::Project => self.data.name = name,
                    RenameTarget::Folder(id) => {
                        if let Some(folder) = self.data.folders.iter_mut().find(|f| f.id == id) {
                            folder.name = name;
                        }
                    }
                    RenameTarget::Request(id) => {
                        if let Some(req) = self.data.find_request_mut(id) {
                            req.name = name;
                        }
                    }
                }
                self.touch();
            }
            self.rename = None;
        } else if close {
            self.rename = None;
        } else {
            self.rename = Some(state);
        }
    }

    fn reset_window(&mut self, ctx: &egui::Context, p: &Palette) {
        if !self.confirm_reset {
            return;
        }
        let mut close = false;
        let mut reset = false;
        egui::Window::new("Reset demo data?")
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .collapsible(false)
            .resizable(false)
            .min_width(380.0)
            .show(ctx, |ui| {
                ui.add_space(2.0);
                ui.label(
                    RichText::new(
                        "Your current collections, requests and history will be replaced by the original demo workspace.",
                    )
                    .size(12.0)
                    .color(p.muted),
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::button(ui, p, "Reset workspace", Btn::Danger).clicked() {
                            reset = true;
                        }
                        if widgets::button(ui, p, "Keep my data", Btn::Secondary).clicked() {
                            close = true;
                        }
                    });
                });
            });
        if reset {
            self.data = crate::model::seed();
            self.responses.clear();
            self.open_tabs.clear();
            self.active_tab = None;
            if let Some(first) = self.data.folders.first().and_then(|f| f.requests.first()) {
                let id = first.id;
                self.open_tabs.push(id);
                self.active_tab = Some(id);
            }
            self.env_selected = self.data.active_env;
            self.sync_session();
            self.touch();
            self.save_now();
            self.confirm_reset = false;
            self.toast(ToastTone::Success, "Demo workspace restored");
        } else if close {
            self.confirm_reset = false;
        }
    }

    /// Parses a cURL command and adds the request to `folder` (or the first
    /// collection), opening it. Returns the new request name.
    fn add_imported_request(
        &mut self,
        text: &str,
        folder: Option<Id>,
    ) -> Result<String, String> {
        let parsed = codegen::parse_curl(text)?;
        let req = parsed.into_request();
        let name = req.name.clone();
        let id = req.id;
        let target = folder.or_else(|| self.data.folders.first().map(|f| f.id));
        let target = match target {
            Some(id) => id,
            None => {
                let collection = Folder::new("Imported");
                let id = collection.id;
                self.data.folders.push(collection);
                id
            }
        };
        if let Some(collection) = self.data.folders.iter_mut().find(|f| f.id == target) {
            collection.open = true;
            collection.requests.push(req);
        }
        self.touch();
        self.open_request(id);
        self.req_tab = ReqTab::Params;
        Ok(name)
    }

    fn snippets_window(&mut self, ctx: &egui::Context, p: &Palette) {
        let Some(mut state) = self.snippets.take() else {
            return;
        };
        let Some(mut req) = self.data.find_request(state.request_id).cloned() else {
            self.snippets = None;
            return;
        };
        if let Some(auth) = self.data.resolved_auth(state.request_id) {
            req.auth = auth;
        }
        let mut close = false;
        let mut copy = false;
        let mut code = String::new();
        egui::Window::new("Code snippets")
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .collapsible(false)
            .resizable(false)
            .min_width(640.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for lang in codegen::Lang::ALL {
                        if small_tab(ui, p, lang.label(), state.lang == lang) {
                            state.lang = lang;
                        }
                    }
                });
                ui.add_space(6.0);
                let env = if state.resolved {
                    self.data.env_map()
                } else {
                    HashMap::new()
                };
                code = codegen::snippet(state.lang, &req, &env);
                Frame::NONE
                    .fill(p.code_bg)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width(620.0);
                        ScrollArea::both()
                            .max_height(320.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&code)
                                            .font(FontId::monospace(11.8))
                                            .color(p.text),
                                    )
                                    .selectable(true)
                                    .wrap_mode(egui::TextWrapMode::Extend),
                                );
                            });
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.checkbox(
                        &mut state.resolved,
                        RichText::new("Resolve environment variables")
                            .size(11.5)
                            .color(p.muted),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::button(ui, p, "Close", Btn::Secondary).clicked() {
                            close = true;
                        }
                        if widgets::button(ui, p, "Copy", Btn::Primary).clicked() {
                            copy = true;
                        }
                    });
                });
            });
        if copy {
            ctx.copy_text(code.clone());
            self.toast(ToastTone::Success, "Snippet copied to clipboard");
        }
        if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.snippets = None;
        } else {
            self.snippets = Some(state);
        }
    }

    fn import_window(&mut self, ctx: &egui::Context, p: &Palette) {
        let Some(mut state) = self.import.take() else {
            return;
        };
        let mut close = false;
        let mut commit = false;
        egui::Window::new("Import from cURL")
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .collapsible(false)
            .resizable(false)
            .min_width(560.0)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(
                        "Paste a cURL command. Method, URL, query parameters, headers, auth and body are imported.",
                    )
                    .size(11.5)
                    .color(p.muted),
                );
                ui.add_space(6.0);
                Frame::NONE
                    .fill(p.code_bg)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::same(6))
                    .show(ui, |ui| {
                        ui.set_width(520.0);
                        ScrollArea::vertical()
                            .id_salt("curl_import_text")
                            .max_height(150.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.add(
                                    TextEdit::multiline(&mut state.text)
                                        .font(FontId::monospace(11.5))
                                        .desired_width(f32::INFINITY)
                                        .desired_rows(5)
                                        .hint_text(
                                            "curl 'https://api.example.com/v1/users' \\\n  -H 'Accept: application/json'",
                                        ),
                                );
                            });
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if widgets::button(ui, p, "Paste", Btn::Secondary).clicked() {
                        match arboard::Clipboard::new().and_then(|mut clip| clip.get_text()) {
                            Ok(text) if !text.trim().is_empty() => {
                                state.text = text;
                                state.error = None;
                            }
                            Ok(_) => {
                                state.error = Some("The clipboard is empty.".to_string());
                            }
                            Err(_) => {
                                state.error =
                                    Some("Could not read the clipboard.".to_string());
                            }
                        }
                    }
                    ui.add_space(4.0);
                    ui.label(RichText::new("Collection").size(11.0).color(p.muted));
                    let selected = state
                        .folder
                        .and_then(|id| self.data.folders.iter().find(|f| f.id == id))
                        .map(|f| f.name.clone())
                        .unwrap_or_else(|| "Choose…".to_string());
                    egui::ComboBox::from_id_salt("import_folder")
                        .width(210.0)
                        .selected_text(RichText::new(selected).size(11.5).color(p.text))
                        .show_ui(ui, |ui| {
                            for folder in &self.data.folders {
                                if ui
                                    .selectable_label(
                                        state.folder == Some(folder.id),
                                        &folder.name,
                                    )
                                    .clicked()
                                {
                                    state.folder = Some(folder.id);
                                }
                            }
                        });
                });
                if let Some(err) = &state.error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(err).size(11.5).color(p.danger));
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::button(ui, p, "Cancel", Btn::Secondary).clicked() {
                            close = true;
                        }
                        let submit = widgets::button(ui, p, "Import", Btn::Primary).clicked()
                            || ui.input(|i| {
                                (i.modifiers.command || i.modifiers.ctrl)
                                    && i.key_pressed(egui::Key::Enter)
                            });
                        if submit {
                            commit = true;
                        }
                    });
                });
            });

        if commit {
            match self.add_imported_request(&state.text, state.folder) {
                Ok(name) => {
                    self.save_now();
                    self.import = None;
                    self.toast(ToastTone::Success, format!("Imported \"{name}\" from cURL"));
                }
                Err(err) => {
                    state.error = Some(err);
                    self.import = Some(state);
                }
            }
        } else if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.import = None;
        } else {
            self.import = Some(state);
        }
    }

    fn auth_editor(
        &mut self,
        ui: &mut Ui,
        p: &Palette,
        auth: &mut Auth,
        allow_inherit: bool,
        collection: Option<(Id, String, Auth)>,
        changed: &mut bool,
    ) {
        let kinds: Vec<&'static str> = if allow_inherit {
            Auth::KINDS.to_vec()
        } else {
            Auth::KINDS[1..].to_vec()
        };
        let base = if allow_inherit { 0 } else { 1 };
        let current = auth.kind_index();
        let mut index = current.saturating_sub(base).min(kinds.len() - 1);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Type").size(12.0).color(p.muted));
            egui::ComboBox::from_id_salt("auth_kind")
                .width(250.0)
                .selected_text(RichText::new(kinds[index]).size(12.0))
                .show_ui(ui, |ui| {
                    for (position, kind) in kinds.iter().enumerate() {
                        if ui.selectable_label(index == position, *kind).clicked() {
                            index = position;
                        }
                    }
                });
        });
        if base + index != current {
            *auth = match (auth.clone(), base + index) {
                (
                    Auth::OAuth2 {
                        token_url,
                        client_id,
                        client_secret,
                        scope,
                        authorization_url,
                        ..
                    },
                    5 | 6,
                ) => Auth::OAuth2 {
                    grant: if base + index == 6 {
                        OAuthGrant::Pkce
                    } else {
                        OAuthGrant::ClientCredentials
                    },
                    token_url,
                    client_id,
                    client_secret,
                    scope,
                    authorization_url,
                },
                _ => Auth::with_kind(base + index),
            };
            *changed = true;
        }
        ui.add_space(10.0);
        let defined = self.defined_vars();
        let oauth_snapshot = if matches!(auth, Auth::OAuth2 { .. }) {
            Some(auth.clone())
        } else {
            None
        };
        match auth {
            Auth::Inherit => {
                if let Some((folder_id, name, folder_auth)) = &collection {
                    ui.label(
                        RichText::new(format!(
                            "Inherits the auth configured on \"{name}\"."
                        ))
                        .size(11.5)
                        .color(p.muted),
                    );
                    ui.add_space(2.0);
                    widgets::chip(
                        ui,
                        p,
                        folder_auth.label(),
                        p.muted,
                        p.panel_alt,
                        false,
                    );
                    ui.add_space(8.0);
                    if widgets::button(ui, p, "Edit collection auth", Btn::Secondary)
                        .clicked()
                    {
                        self.folder_auth = Some(FolderAuthState {
                            folder_id: *folder_id,
                            name: name.clone(),
                            auth: folder_auth.clone(),
                        });
                    }
                } else {
                    ui.label(
                        RichText::new(
                            "This request inherits the auth of its collection.",
                        )
                        .size(11.5)
                        .color(p.faint),
                    );
                }
            }
            Auth::None => {
                ui.label(
                    RichText::new(
                        "This request is sent without authorization headers.",
                    )
                    .size(11.5)
                    .color(p.faint),
                );
            }
            Auth::Bearer { token } => {
                secret_field(ui, p, "Token", token, "{{token}}", changed);
            }
            Auth::Basic { user, pass } => {
                text_field(ui, p, "Username", user, "your-user", &defined, changed);
                secret_field(ui, p, "Password", pass, "your-password", changed);
            }
            Auth::ApiKey { header, value } => {
                text_field(ui, p, "Header name", header, "X-API-Key", &defined, changed);
                secret_field(ui, p, "Key", value, "{{apiKey}}", changed);
            }
            Auth::OAuth2 {
                grant,
                token_url,
                client_id,
                client_secret,
                scope,
                authorization_url,
            } => {
                let pkce = *grant == OAuthGrant::Pkce;
                let auth_snapshot = oauth_snapshot.clone().unwrap_or(Auth::None);
                let env = self.data.env_map();
                let connected = http::oauth_connected(&auth_snapshot, &env, &self.oauth);
                ui.horizontal(|ui| {
                    if connected {
                        widgets::chip(
                            ui,
                            p,
                            "Connected",
                            p.success,
                            p.success_soft,
                            false,
                        );
                    } else {
                        widgets::chip(
                            ui,
                            p,
                            "Not connected",
                            p.muted,
                            p.panel_alt,
                            false,
                        );
                    }
                    if pkce {
                        if self.oauth_pending.is_some() {
                            ui.label(
                                RichText::new("Waiting for the browser…")
                                    .size(10.5)
                                    .color(p.muted),
                            );
                        } else if widgets::button(ui, p, "Connect", Btn::Primary)
                            .clicked()
                        {
                            self.start_oauth_pkce(auth_snapshot.clone());
                        }
                    }
                });
                ui.add_space(6.0);
                if pkce {
                    text_field(
                        ui,
                        p,
                        "Authorization URL",
                        authorization_url,
                        "https://provider.com/authorize",
                        &defined,
                        changed,
                    );
                }
                text_field(
                    ui,
                    p,
                    "Token URL",
                    token_url,
                    "https://provider.com/token",
                    &defined,
                    changed,
                );
                text_field(ui, p, "Client ID", client_id, "client-id", &defined, changed);
                secret_field(
                    ui,
                    p,
                    "Client secret",
                    client_secret,
                    "client-secret",
                    changed,
                );
                text_field(ui, p, "Scope", scope, "read write", &defined, changed);
                if !pkce {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "A client-credentials token is requested automatically before each send.",
                        )
                        .size(10.5)
                        .color(p.faint),
                    );
                }
            }
        }
    }

    fn open_publish(&mut self) {
        self.publish = Some(PublishState {
            title: format!("{} API", self.data.name),
            intro: String::new(),
            base_url: self.data.settings.docs_base_url.clone(),
            include_examples: true,
            include_bodies: true,
            include_auth: true,
            dark: self.data.settings.dark,
            resolve_variables: true,
            published: None,
        });
    }

    fn publish_docs(&mut self, state: &PublishState) -> Result<String, String> {
        let options = docs::Options {
            title: state.title.clone(),
            intro: state.intro.clone(),
            base_url: state.base_url.clone(),
            include_examples: state.include_examples,
            include_bodies: state.include_bodies,
            include_auth: state.include_auth,
            dark: state.dark,
            resolve_variables: state.resolve_variables,
        };
        let site = docs::build(&self.data, &options);
        let Some(folder) = rfd::FileDialog::new()
            .set_title("Choose a folder for the documentation site")
            .pick_folder()
        else {
            return Err("cancelled".to_string());
        };
        std::fs::create_dir_all(&folder)
            .map_err(|error| format!("Could not create {}: {error}", folder.display()))?;
        let file = folder.join("index.html");
        std::fs::write(&file, &site.html)
            .map_err(|error| format!("Could not write {}: {error}", file.display()))?;
        Ok(file.display().to_string())
    }

    fn publish_window(&mut self, ctx: &egui::Context, p: &Palette) {
        let Some(mut state) = self.publish.take() else {
            return;
        };
        let mut close = false;
        let mut publish = false;
        let mut open_site = false;
        let mut copy_link = false;
        let preview_options = docs::Options {
            title: state.title.clone(),
            intro: state.intro.clone(),
            base_url: state.base_url.clone(),
            include_examples: state.include_examples,
            include_bodies: state.include_bodies,
            include_auth: state.include_auth,
            dark: state.dark,
            resolve_variables: state.resolve_variables,
        };
        let anchors = docs::anchors(&self.data);
        egui::Window::new("Publish documentation")
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .collapsible(false)
            .resizable(false)
            .min_width(660.0)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(
                        "Export a read-only documentation site as a single index.html that can be published anywhere.",
                    )
                    .size(11.5)
                    .color(p.muted),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [96.0, 26.0],
                        egui::Label::new(RichText::new("Title").size(12.0).color(p.muted)),
                    );
                    ui.add_sized(
                        [520.0, 26.0],
                        TextEdit::singleline(&mut state.title)
                            .font(FontId::proportional(12.0))
                            .margin(Margin::symmetric(6, 4)),
                    );
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [96.0, 26.0],
                        egui::Label::new(RichText::new("Intro").size(12.0).color(p.muted)),
                    );
                    ui.add_sized(
                        [520.0, 26.0],
                        TextEdit::singleline(&mut state.intro)
                            .hint_text("One line describing the API")
                            .font(FontId::proportional(12.0))
                            .margin(Margin::symmetric(6, 4)),
                    );
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [96.0, 26.0],
                        egui::Label::new(
                            RichText::new("Public URL").size(12.0).color(p.muted),
                        ),
                    );
                    ui.add_sized(
                        [520.0, 26.0],
                        TextEdit::singleline(&mut state.base_url)
                            .hint_text("https://docs.example.com/api")
                            .font(FontId::monospace(11.0))
                            .margin(Margin::symmetric(6, 4)),
                    );
                });
                ui.add_space(2.0);
                ui.label(
                    RichText::new(
                        "Where the site will live. Share links and the per-endpoint copy buttons use it.",
                    )
                    .size(10.5)
                    .color(p.faint),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    let mut examples = state.include_examples;
                    if ui.checkbox(&mut examples, "Example responses").changed() {
                        state.include_examples = examples;
                    }
                    let mut bodies = state.include_bodies;
                    if ui.checkbox(&mut bodies, "Request bodies").changed() {
                        state.include_bodies = bodies;
                    }
                    let mut auth = state.include_auth;
                    if ui.checkbox(&mut auth, "Auth summary").changed() {
                        state.include_auth = auth;
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    let mut dark = state.dark;
                    if ui.checkbox(&mut dark, "Dark theme").changed() {
                        state.dark = dark;
                    }
                    let mut resolved = state.resolve_variables;
                    if ui
                        .checkbox(&mut resolved, "Resolve {{variables}}")
                        .changed()
                    {
                        state.resolve_variables = resolved;
                    }
                    ui.label(
                        RichText::new("secret values are always redacted")
                            .size(10.5)
                            .color(p.faint),
                    );
                });
                ui.add_space(12.0);
                Frame::NONE
                    .fill(p.code_bg)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_width(620.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(preview_options.title.trim().to_string())
                                    .size(12.5)
                                    .family(theme::medium_family())
                                    .color(p.text),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "{} endpoints · {} collections",
                                    anchors.len(),
                                    self.data
                                        .folders
                                        .iter()
                                        .filter(|folder| !folder.requests.is_empty())
                                        .count()
                                ))
                                .size(10.5)
                                .color(p.faint),
                            );
                        });
                        ui.add_space(4.0);
                        ScrollArea::vertical()
                            .id_salt("publish_preview")
                            .max_height(120.0)
                            .show(ui, |ui| {
                                for (id, anchor) in &anchors {
                                    let name = self
                                        .data
                                        .find_request(*id)
                                        .map(|request| request.name.clone())
                                        .unwrap_or_default();
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(&name)
                                                .size(11.0)
                                                .color(p.muted),
                                        );
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new(format!("#{anchor}"))
                                                        .size(10.0)
                                                        .monospace()
                                                        .color(p.faint),
                                                );
                                            },
                                        );
                                    });
                                }
                            });
                    });
                ui.add_space(8.0);
                if let Some(path) = &state.published {
                    ui.horizontal(|ui| {
                        widgets::chip(
                            ui,
                            p,
                            "published",
                            p.success,
                            p.success_soft,
                            false,
                        );
                        ui.label(
                            RichText::new(path).size(10.5).monospace().color(p.muted),
                        );
                    });
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::button(ui, p, "Publish…", Btn::Primary).clicked() {
                            publish = true;
                        }
                        if state.published.is_some() {
                            if widgets::button(ui, p, "Open site", Btn::Secondary).clicked() {
                                open_site = true;
                            }
                        }
                        if widgets::button(ui, p, "Copy a share link", Btn::Secondary).clicked()
                        {
                            copy_link = true;
                        }
                        if widgets::button(ui, p, "Close", Btn::Ghost).clicked() {
                            close = true;
                        }
                    });
                });
            });

        if publish {
            match self.publish_docs(&state) {
                Ok(path) => {
                    self.data.settings.docs_base_url = state.base_url.clone();
                    self.touch();
                    self.save_now();
                    self.toast(
                        ToastTone::Success,
                        format!("Documentation published to {path}"),
                    );
                    state.published = Some(path);
                }
                Err(error) if error == "cancelled" => {}
                Err(error) => self.toast(ToastTone::Danger, error),
            }
        }
        if open_site {
            if let Some(path) = &state.published {
                let _ = std::process::Command::new("open").arg(path).spawn();
            }
        }
        if copy_link {
            let base = state.base_url.trim().trim_end_matches('/');
            let link = match anchors.first() {
                Some((_, anchor)) if base.is_empty() => format!("index.html#{anchor}"),
                Some((_, anchor)) => format!("{base}/index.html#{anchor}"),
                None => String::new(),
            };
            if !link.is_empty() {
                self.pending_copy = Some(link);
                self.toast(ToastTone::Success, "Share link copied");
            }
            self.data.settings.docs_base_url = state.base_url.clone();
        }
        if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.publish = None;
        } else {
            self.publish = Some(state);
        }
    }

    fn folder_auth_window(&mut self, ctx: &egui::Context, p: &Palette) {
        let Some(mut state) = self.folder_auth.take() else {
            return;
        };
        let mut save = false;
        let mut close = false;
        let mut changed = false;
        egui::Window::new("Collection auth")
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .collapsible(false)
            .resizable(false)
            .min_width(600.0)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(format!(
                        "Requests in \"{}\" set to Inherit use this auth.",
                        state.name
                    ))
                    .size(11.5)
                    .color(p.muted),
                );
                ui.add_space(8.0);
                self.auth_editor(ui, p, &mut state.auth, false, None, &mut changed);
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::button(ui, p, "Save", Btn::Primary).clicked() {
                            save = true;
                        }
                        if widgets::button(ui, p, "Cancel", Btn::Secondary).clicked() {
                            close = true;
                        }
                    });
                });
            });
        if save {
            if let Some(folder) = self.data.folders.iter_mut().find(|f| f.id == state.folder_id) {
                folder.auth = state.auth.clone();
            }
            self.touch();
            self.save_now();
            self.folder_auth = None;
            self.toast(ToastTone::Success, "Collection auth updated");
        } else if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.folder_auth = None;
        } else {
            self.folder_auth = Some(state);
        }
    }

    fn start_oauth_pkce(&mut self, auth: Auth) {
        if self.oauth_pending.is_some() {
            self.toast(ToastTone::Neutral, "An OAuth login is already in progress");
            return;
        }
        let Auth::OAuth2 {
            authorization_url,
            token_url,
            client_id,
            scope,
            ..
        } = &auth
        else {
            return;
        };
        let env = self.data.env_map();
        let authorization_url = expand_with(authorization_url.trim(), &env);
        let token_url = expand_with(token_url.trim(), &env);
        let client_id = expand_with(client_id.trim(), &env);
        let scope = expand_with(scope.trim(), &env);
        if authorization_url.is_empty() || token_url.is_empty() || client_id.is_empty() {
            self.toast(
                ToastTone::Danger,
                "Set the authorization URL, token URL and client ID first",
            );
            return;
        }
        let listener = match std::net::TcpListener::bind(("127.0.0.1", 0)) {
            Ok(listener) => listener,
            Err(error) => {
                self.toast(
                    ToastTone::Danger,
                    format!("Could not start the local OAuth listener: {error}"),
                );
                return;
            }
        };
        let port = listener
            .local_addr()
            .map(|address| address.port())
            .unwrap_or(0);
        let redirect = format!("http://127.0.0.1:{port}/callback");
        let (verifier, challenge, state) = oauth::pkce_pair();
        let mut url = match reqwest::Url::parse(&authorization_url) {
            Ok(url) => url,
            Err(error) => {
                self.toast(
                    ToastTone::Danger,
                    format!("Invalid authorization URL: {error}"),
                );
                return;
            }
        };
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("response_type", "code");
            query.append_pair("client_id", &client_id);
            query.append_pair("redirect_uri", &redirect);
            query.append_pair("state", &state);
            query.append_pair("code_challenge", &challenge);
            query.append_pair("code_challenge_method", "S256");
            if !scope.is_empty() {
                query.append_pair("scope", &scope);
            }
        }
        let opened = std::process::Command::new("open")
            .arg(url.as_str())
            .spawn()
            .is_ok()
            || std::process::Command::new("xdg-open")
                .arg(url.as_str())
                .spawn()
                .is_ok();
        if !opened {
            self.pending_copy = Some(url.to_string());
            self.toast(
                ToastTone::Danger,
                "Could not open the browser; the authorization URL was copied",
            );
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.oauth_pending = Some(rx);
        let cache = self.oauth.clone();
        let settings = self.data.settings.clone();
        std::thread::spawn(move || {
            let result = oauth::wait_for_callback(
                listener,
                &state,
                std::time::Duration::from_secs(180),
            )
            .and_then(|code| {
                http::exchange_pkce_code(
                    &auth,
                    &env,
                    &cache,
                    &code,
                    &verifier,
                    &redirect,
                    &settings,
                )
            });
            let _ = tx.send(result);
        });
        self.toast(ToastTone::Neutral, "Complete the login in your browser…");
    }

    fn poll_oauth(&mut self) {
        let mut received: Option<Result<(), String>> = None;
        let mut disconnected = false;
        if let Some(rx) = &self.oauth_pending {
            match rx.try_recv() {
                Ok(result) => received = Some(result),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => disconnected = true,
            }
        }
        if let Some(result) = received {
            self.oauth_pending = None;
            match result {
                Ok(()) => self.toast(ToastTone::Success, "OAuth2 connected"),
                Err(error) => self.toast(ToastTone::Danger, error),
            }
        } else if disconnected {
            self.oauth_pending = None;
        }
    }

    fn capture_dialog_window(&mut self, ctx: &egui::Context, p: &Palette) {
        let Some(mut state) = self.capture_dialog.take() else {
            return;
        };
        let Some(request_name) = self
            .data
            .find_request(state.request_id)
            .map(|req| req.name.clone())
        else {
            return;
        };
        let mut close = false;
        let mut commit = false;
        egui::Window::new("Capture as variable")
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .collapsible(false)
            .resizable(false)
            .min_width(440.0)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(format!(
                        "Save a value from the response of “{request_name}” into the active environment."
                    ))
                    .size(11.5)
                    .color(p.muted),
                );
                ui.add_space(8.0);
                ui.label(RichText::new("JSON path").size(10.0).color(p.faint));
                ui.add_space(2.0);
                Frame::NONE
                    .fill(p.code_bg)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(7))
                    .inner_margin(Margin::symmetric(8, 6))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(&state.path)
                                .font(FontId::monospace(11.5))
                                .color(p.json_key),
                        );
                    });
                ui.add_space(8.0);
                ui.label(RichText::new("Variable name").size(10.0).color(p.faint));
                ui.add_space(2.0);
                let resp = ui.add_sized(
                    [ui.available_width(), 30.0],
                    TextEdit::singleline(&mut state.variable)
                        .font(FontId::monospace(12.0))
                        .margin(Margin::symmetric(8, 4)),
                );
                resp.request_focus();
                let variable = sanitize_variable(&state.variable);
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "Reference it as {}{}{} in later requests.",
                        "{{", variable, "}}"
                    ))
                    .size(10.5)
                    .color(p.faint),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::button(ui, p, "Capture", Btn::Primary).clicked()
                            || ui.input(|i| i.key_pressed(egui::Key::Enter))
                        {
                            commit = true;
                        }
                        if widgets::button(ui, p, "Cancel", Btn::Secondary).clicked()
                            || ui.input(|i| i.key_pressed(egui::Key::Escape))
                        {
                            close = true;
                        }
                    });
                });
            });

        if commit {
            let variable = sanitize_variable(&state.variable);
            if let Some(request) = self.data.find_request_mut(state.request_id) {
                if let Some(existing) = request
                    .captures
                    .iter_mut()
                    .find(|capture| capture.variable == variable)
                {
                    existing.path = state.path.clone();
                    existing.source = CaptureSource::JsonField;
                    existing.enabled = true;
                } else {
                    request.captures.push(Capture::new(
                        CaptureSource::JsonField,
                        &state.path,
                        &variable,
                    ));
                }
            }
            self.touch();
            self.toast(
                ToastTone::Success,
                format!(
                    "Will capture {}{}{} after the next send",
                    "{{", variable, "}}"
                ),
            );
            self.capture_dialog = None;
        } else if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.capture_dialog = None;
        } else {
            self.capture_dialog = Some(state);
        }
    }

    fn parse_api_import(&mut self) {
        let Some(state) = self.api_import.as_mut() else {
            return;
        };
        if state.text.trim().is_empty() {
            state.plan = None;
            state.error = None;
            return;
        }
        match importer::detect_and_parse(&state.text) {
            Ok(plan) => {
                state.plan = Some(plan);
                state.error = None;
            }
            Err(error) => {
                state.plan = None;
                state.error = Some(error);
            }
        }
    }

    /// Creates a GET request from a URL (CLI argument, paste or dropped link).
    fn ingest_url(&mut self, raw: &str) -> bool {
        let token = raw
            .split_whitespace()
            .find(|part| part.starts_with("http://") || part.starts_with("https://"));
        let Some(url) = token else {
            return false;
        };
        let url = url
            .trim()
            .trim_end_matches(['"', '\'', '>', ')', ']', ',', ';'])
            .to_string();
        if reqwest::Url::parse(&url).is_err() {
            return false;
        }
        let name = reqwest::Url::parse(&url)
            .ok()
            .map(|parsed| {
                parsed
                    .path_segments()
                    .and_then(|mut segments| segments.next_back())
                    .filter(|segment| !segment.is_empty())
                    .map(|segment| segment.to_string())
                    .or_else(|| parsed.host_str().map(|host| host.to_string()))
                    .unwrap_or_else(|| "New request".to_string())
            })
            .unwrap_or_else(|| "New request".to_string());
        let folder_id = match self.data.folders.first() {
            Some(folder) => folder.id,
            None => {
                let folder = Folder::new("Requests");
                let id = folder.id;
                self.data.folders.push(folder);
                id
            }
        };
        let request = ApiRequest::new(&name, Method::Get, &url);
        let request_id = request.id;
        if let Some(folder) = self.data.folders.iter_mut().find(|f| f.id == folder_id) {
            folder.open = true;
            folder.requests.push(request);
        }
        self.open_request(request_id);
        self.touch();
        self.toast(ToastTone::Success, format!("Request created from {name}"));
        true
    }

    fn handle_url_paste(&mut self, ctx: &egui::Context) {
        if ctx.memory(|memory| memory.focused().is_some()) {
            return;
        }
        let pasted = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Paste(text) => Some(text.clone()),
                _ => None,
            })
        });
        if let Some(text) = pasted {
            if self.ingest_url(&text) {
                ctx.input_mut(|input| {
                    input.events.retain(|event| {
                        !matches!(event, egui::Event::Paste(_))
                    });
                });
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let mut dropped_url: Option<String> = None;
        ctx.input(|input| {
            for file in &input.raw.dropped_files {
                let extension = file
                    .path
                    .as_ref()
                    .and_then(|path| path.extension())
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if extension == "webloc" || extension == "url" {
                    if let Some(path) = &file.path {
                        if let Ok(text) = std::fs::read_to_string(path) {
                            dropped_url = text
                                .split_whitespace()
                                .find(|part| {
                                    part.starts_with("http://")
                                        || part.starts_with("https://")
                                })
                                .map(|part| {
                                    part.trim_matches(|ch| {
                                        ch == '<' || ch == '>' || ch == '"'
                                    })
                                    .to_string()
                                });
                        }
                    }
                } else if let Some(bytes) = &file.bytes {
                    if let Ok(text) = std::str::from_utf8(bytes) {
                        if text.trim().starts_with("http://")
                            || text.trim().starts_with("https://")
                        {
                            dropped_url = Some(text.trim().to_string());
                        }
                    }
                }
            }
        });
        if let Some(url) = dropped_url {
            self.ingest_url(&url);
            return;
        }
        let path = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .find_map(|file| file.path.clone())
        });
        let Some(path) = path else {
            return;
        };
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "json" | "yaml" | "yml") {
            return;
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                self.api_import = Some(ApiImportState {
                    text,
                    source: path
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    plan: None,
                    error: None,
                });
                self.parse_api_import();
            }
            Err(error) => self.toast(
                ToastTone::Danger,
                format!("Could not read the dropped file: {error}"),
            ),
        }
    }

    /// Applies a parsed import plan to the workspace and returns the toast
    /// message. Shared by the dialog and the screenshot harness.
    fn apply_import_plan(&mut self, plan: ImportPlan) -> String {
        let ImportPlan {
            format,
            title,
            folders,
            variables,
            warnings,
        } = plan;
        let count: usize = folders.iter().map(|folder| folder.requests.len()).sum();
        let mut first_request: Option<Id> = None;
        for folder in folders {
            if first_request.is_none() {
                first_request = folder.requests.first().map(|request| request.id);
            }
            self.data.folders.push(folder);
        }
        if !variables.is_empty() {
            let env_id = match self.data.active_env {
                Some(id) if self.data.envs.iter().any(|env| env.id == id) => id,
                _ => {
                    let mut env = crate::model::Environment::new(&format!("{title} variables"));
                    env.vars.clear();
                    let id = env.id;
                    self.data.envs.push(env);
                    self.data.active_env = Some(id);
                    self.env_selected = Some(id);
                    id
                }
            };
            if let Some(env) = self.data.envs.iter_mut().find(|env| env.id == env_id) {
                for variable in variables {
                    if !env
                        .vars
                        .iter()
                        .any(|existing| existing.key == variable.key)
                    {
                        env.vars.push(variable);
                    }
                }
            }
        }
        self.touch();
        self.save_now();
        if let Some(id) = first_request {
            self.open_request(id);
            self.req_tab = ReqTab::Params;
            self.nav = Nav::Apis;
        }
        let mut message = format!("Imported {count} requests from {format}");
        if !warnings.is_empty() {
            message.push_str(&format!(" · {} warnings", warnings.len()));
        }
        message
    }

    fn api_import_window(&mut self, ctx: &egui::Context, p: &Palette) {
        let Some(mut state) = self.api_import.take() else {
            return;
        };
        let mut close = false;
        let mut commit = false;
        let mut changed = false;

        egui::Window::new("Import API")
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .collapsible(false)
            .resizable(false)
            .min_width(640.0)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(
                        "OpenAPI 3 (JSON or YAML) or a Postman collection (JSON). Paste it, choose a file, or drop it on the window.",
                    )
                    .size(11.5)
                    .color(p.muted),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if widgets::button(ui, p, "Choose file…", Btn::Secondary).clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("API spec", &["json", "yaml", "yml"])
                            .pick_file()
                        {
                            match std::fs::read_to_string(&path) {
                                Ok(text) => {
                                    state.text = text;
                                    state.source = path
                                        .file_name()
                                        .map(|name| name.to_string_lossy().to_string())
                                        .unwrap_or_default();
                                    changed = true;
                                }
                                Err(error) => {
                                    state.error =
                                        Some(format!("Could not read the file: {error}"));
                                }
                            }
                        }
                    }
                    if widgets::button(ui, p, "Paste", Btn::Secondary).clicked() {
                        match arboard::Clipboard::new().and_then(|mut clip| clip.get_text()) {
                            Ok(text) if !text.trim().is_empty() => {
                                state.text = text;
                                state.source = "Clipboard".to_string();
                                changed = true;
                            }
                            Ok(_) => {
                                state.error = Some("The clipboard is empty.".to_string());
                            }
                            Err(_) => {
                                state.error =
                                    Some("Could not read the clipboard.".to_string());
                            }
                        }
                    }
                    if !state.source.is_empty() {
                        widgets::chip(ui, p, &state.source, p.muted, p.panel_alt, false);
                    }
                });
                ui.add_space(8.0);
                Frame::NONE
                    .fill(p.code_bg)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(600.0);
                        let mut edited = false;
                        ScrollArea::vertical()
                            .id_salt("api_import_text")
                            .max_height(208.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let edit = ui.add(
                                    TextEdit::multiline(&mut state.text)
                                        .font(FontId::monospace(11.0))
                                        .desired_width(f32::INFINITY)
                                        .desired_rows(8)
                                        .hint_text(
                                            "Paste an OpenAPI document or a Postman collection…",
                                        ),
                                );
                                edited = edit.changed();
                            });
                        if edited {
                            state.source = "Pasted text".to_string();
                            changed = true;
                        }
                    });

                if changed {
                    if state.text.trim().is_empty() {
                        state.plan = None;
                        state.error = None;
                    } else {
                        match importer::detect_and_parse(&state.text) {
                            Ok(plan) => {
                                state.plan = Some(plan);
                                state.error = None;
                            }
                            Err(error) => {
                                state.plan = None;
                                state.error = Some(error);
                            }
                        }
                    }
                }

                ui.add_space(8.0);
                if let Some(error) = &state.error {
                    ui.label(RichText::new(error).size(11.0).color(p.danger));
                } else if let Some(plan) = &state.plan {
                    ui.label(RichText::new(plan.summary()).size(11.5).color(p.success));
                    for warning in plan.warnings.iter().take(3) {
                        ui.label(
                            RichText::new(format!("• {warning}"))
                                .size(10.5)
                                .color(p.warn),
                        );
                    }
                    if plan.warnings.len() > 3 {
                        ui.label(
                            RichText::new(format!(
                                "+ {} more warnings",
                                plan.warnings.len() - 3
                            ))
                            .size(10.5)
                            .color(p.warn),
                        );
                    }
                } else {
                    ui.label(
                        RichText::new("Waiting for a document…")
                            .size(10.5)
                            .color(p.faint),
                    );
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_enabled_ui(state.plan.is_some(), |ui| {
                            if widgets::button(ui, p, "Import", Btn::Primary).clicked() {
                                commit = true;
                            }
                        });
                        if widgets::button(ui, p, "Cancel", Btn::Secondary).clicked() {
                            close = true;
                        }
                    });
                });
            });

        if commit {
            if let Some(plan) = state.plan.take() {
                let message = self.apply_import_plan(plan);
                self.api_import = None;
                self.toast(ToastTone::Success, message);
                return;
            }
        }
        if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.api_import = None;
        } else {
            self.api_import = Some(state);
        }
    }

    fn export_window(&mut self, ctx: &egui::Context, p: &Palette) {
        let Some(mut state) = self.export.take() else {
            return;
        };
        let mut close = false;
        let mut copy = false;
        let mut save = false;
        egui::Window::new("Export workspace")
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .collapsible(false)
            .resizable(false)
            .min_width(700.0)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(
                        "Generate an OpenAPI 3 document or a Postman collection from this workspace.",
                    )
                    .size(11.5)
                    .color(p.muted),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let formats = ExportFormat::ALL;
                    let labels: Vec<&str> =
                        formats.iter().map(|format| format.label()).collect();
                    let mut index = formats
                        .iter()
                        .position(|format| *format == state.format)
                        .unwrap_or(0);
                    if widgets::segmented(ui, p, &labels, &mut index) {
                        let format = formats
                            .get(index)
                            .copied()
                            .unwrap_or(ExportFormat::OpenApi);
                        state.format = format;
                        state.doc = exporter::export(&self.data, format);
                        state.saved = None;
                    }
                    if let Some(path) = &state.saved {
                        ui.label(
                            RichText::new(format!("Saved to {path}"))
                                .size(10.5)
                                .color(p.success),
                        );
                    }
                });
                ui.add_space(6.0);
                for warning in state.doc.warnings.iter().take(4) {
                    ui.label(
                        RichText::new(format!("• {warning}"))
                            .size(10.5)
                            .color(p.warn),
                    );
                }
                ui.add_space(6.0);
                Frame::NONE
                    .fill(p.code_bg)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width(660.0);
                        ScrollArea::both()
                            .id_salt("export_preview")
                            .max_height(300.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&state.doc.text)
                                            .font(FontId::monospace(10.8))
                                            .color(p.text),
                                    )
                                    .selectable(true)
                                    .wrap_mode(egui::TextWrapMode::Extend),
                                );
                            });
                    });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::button(ui, p, "Save file…", Btn::Primary).clicked() {
                            save = true;
                        }
                        if widgets::button(ui, p, "Copy", Btn::Secondary).clicked() {
                            copy = true;
                        }
                        if widgets::button(ui, p, "Close", Btn::Ghost).clicked() {
                            close = true;
                        }
                    });
                });
            });

        if copy {
            ctx.copy_text(state.doc.text.clone());
            self.toast(ToastTone::Success, "Export copied to clipboard");
        }
        if save {
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name(state.format.file_name())
                .add_filter("JSON", &["json"])
                .save_file()
            {
                match std::fs::write(&path, &state.doc.text) {
                    Ok(()) => {
                        state.saved = Some(path.display().to_string());
                        self.toast(
                            ToastTone::Success,
                            format!("Exported to {}", path.display()),
                        );
                    }
                    Err(error) => {
                        self.toast(
                            ToastTone::Danger,
                            format!("Could not write the file: {error}"),
                        );
                    }
                }
            }
        }
        if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.export = None;
        } else {
            self.export = Some(state);
        }
    }

    // -----------------------------------------------------------------------
    // Keyboard + screenshot
    // -----------------------------------------------------------------------

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let (cmd, shift, enter, s, w, n, c, i, z) = ctx.input(|i| {
            (
                i.modifiers.command || i.modifiers.ctrl,
                i.modifiers.shift,
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::S),
                i.key_pressed(egui::Key::W),
                i.key_pressed(egui::Key::N),
                i.key_pressed(egui::Key::C),
                i.key_pressed(egui::Key::I),
                i.key_pressed(egui::Key::Z),
            )
        });
        if !cmd {
            return;
        }
        if z {
            let modal_open = self.import.is_some()
                || self.api_import.is_some()
                || self.export.is_some()
                || self.rename.is_some()
                || self.snippets.is_some()
                || self.capture_dialog.is_some()
                || self.confirm_reset;
            // Leave undo inside text fields to egui's own editor.
            if !modal_open && ctx.memory(|memory| memory.focused().is_none()) {
                if shift {
                    self.redo();
                } else {
                    self.undo();
                }
            }
            return;
        }
        if shift && c {
            if let Some(id) = self.active_tab {
                self.apply_side_action(SideAction::CopyCurl(id));
            }
            return;
        }
        if shift && i {
            self.apply_side_action(SideAction::ImportCurl(None));
            return;
        }
        if enter {
            if let Some(id) = self.active_tab {
                if self.sending != Some(id) {
                    if let Some(req) = self.data.find_request(id).cloned() {
                        self.start_send(req);
                    }
                }
            }
        }
        if s {
            self.save_now();
            self.toast(ToastTone::Success, "Workspace saved");
        }
        if w {
            if let Some(id) = self.active_tab {
                self.close_tab(id);
            }
        }
        if n {
            self.apply_side_action(SideAction::NewRequest(None));
        }
    }

    fn screenshot_logic(&mut self, ctx: &egui::Context) {
        let Some(mut shot) = self.shot.take() else {
            return;
        };
        shot.frames += 1;
        if shot.frames >= 120 && !shot.requested {
            shot.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let mut captured: Option<Arc<egui::ColorImage>> = None;
        ctx.input(|i| {
            for event in &i.raw.events {
                if let egui::Event::Screenshot { image, .. } = event {
                    captured = Some(image.clone());
                }
            }
        });
        if let Some(image) = captured {
            let width = image.size[0] as u32;
            let height = image.size[1] as u32;
            let pixels: Vec<u8> = image
                .pixels
                .iter()
                .flat_map(|p| [p.r(), p.g(), p.b(), p.a()])
                .collect();
            if let Some(buffer) = image::RgbaImage::from_raw(width, height, pixels) {
                if let Some(parent) = shot.path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = buffer.save(&shot.path);
            }
            let _ = crate::model::save(&self.data);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        self.shot = Some(shot);
        ctx.request_repaint();
    }
}

impl eframe::App for LucApiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) {
            self.sync_session();
            let _ = crate::model::save(&self.data);
        }
        self.poll_http();
        self.poll_tests();
        self.poll_oauth();
        self.handle_native_menu();
        self.shortcuts(ctx);
        self.sync_mock();
        self.handle_dropped_files(ctx);
        self.handle_url_paste(ctx);

        let p = self.palette();

        egui::TopBottomPanel::top("topbar")
            .exact_height(48.0)
            .frame(Frame::NONE.fill(p.panel))
            .show(ctx, |ui| self.top_bar(ui, &p));

        egui::TopBottomPanel::bottom("statusbar")
            .exact_height(26.0)
            .frame(Frame::NONE.fill(p.panel))
            .show(ctx, |ui| self.status_bar(ui, &p));

        egui::SidePanel::left("rail")
            .exact_width(54.0)
            .resizable(false)
            .frame(Frame::NONE.fill(p.panel))
            .show(ctx, |ui| self.rail(ui, &p));

        if self.nav == Nav::Apis {
            egui::SidePanel::left("collection")
                .exact_width(270.0)
                .resizable(false)
                .frame(Frame::NONE.fill(p.panel))
                .show(ctx, |ui| self.sidebar(ui, &p));
        }

        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(p.bg))
            .show(ctx, |ui| match self.nav {
                Nav::Apis => self.api_view(ui, &p),
                Nav::History => self.history_view(ui, &p),
                Nav::Environments => self.environments_view(ui, &p),
                Nav::Tests => self.tests_view(ui, &p),
                Nav::Mock => self.mock_view(ui, &p),
                Nav::Docs => self.docs_view(ui, &p),
                Nav::Settings => self.settings_view(ui, &p),
            });

        if let Some(text) = self.pending_copy.take() {
            ctx.copy_text(text);
        }
        self.toast_ui(ctx, &p);
        self.rename_window(ctx, &p);
        self.reset_window(ctx, &p);
        self.snippets_window(ctx, &p);
        self.import_window(ctx, &p);
        self.capture_dialog_window(ctx, &p);
        self.folder_auth_window(ctx, &p);
        self.publish_window(ctx, &p);
        self.api_import_window(ctx, &p);
        self.export_window(ctx, &p);

        if self.dirty && self.last_edit.elapsed() > Duration::from_millis(900) {
            self.history.commit(&self.data);
            let _ = crate::model::save(&self.data);
            self.dirty = false;
            self.last_saved = Some(Instant::now());
        }

        self.screenshot_logic(ctx);
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn elide(painter: &egui::Painter, text: &str, font: &FontId, max_width: f32) -> String {
    let full = painter.layout_no_wrap(text.to_string(), font.clone(), Color32::WHITE);
    if full.size().x <= max_width {
        return text.to_string();
    }
    let mut out = String::new();
    for ch in text.chars() {
        let candidate = format!("{out}{ch}…");
        let galley = painter.layout_no_wrap(candidate.clone(), font.clone(), Color32::WHITE);
        if galley.size().x > max_width {
            break;
        }
        out.push(ch);
    }
    format!("{out}…")
}

fn suggest_variable(path: &str) -> String {
    let last = path
        .rsplit(['.', '[', ']'])
        .find(|part| !part.trim().is_empty())
        .unwrap_or("value")
        .trim_matches('"')
        .trim_matches('\'');
    sanitize_variable(last)
}

fn sanitize_variable(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        return "value".to_string();
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("v_{out}")
    } else {
        out
    }
}

fn count_suffix(count: usize) -> String {
    if count == 0 {
        String::new()
    } else {
        format!(" {count}")
    }
}

fn body_index(mode: BodyMode) -> usize {
    match mode {
        BodyMode::None => 0,
        BodyMode::Json => 1,
        BodyMode::Text => 2,
        BodyMode::Form => 3,
        BodyMode::Multipart => 4,
        BodyMode::Binary => 5,
    }
}

fn send_button(ui: &mut Ui, p: &Palette, loading: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(92.0, 32.0), Sense::click());
    let hovered = resp.hovered();
    let bg = if loading {
        theme::mix(p.accent, p.panel, 0.35)
    } else if hovered {
        p.accent_hover
    } else {
        p.accent
    };
    ui.painter().rect_filled(rect, CornerRadius::same(7), bg);
    let icon_rect =
        egui::Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), Vec2::splat(13.0));
    widgets::paint_icon(ui.painter(), icon_rect, Icon::Send, p.on_accent, 1.5);
    ui.painter().text(
        pos2(rect.left() + 52.0, rect.center().y + 0.5),
        Align2::CENTER_CENTER,
        if loading { "Sending" } else { "Send" },
        FontId::proportional(12.5),
        p.on_accent,
    );
    if hovered && !loading {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp
}

fn underline_tab(ui: &mut Ui, p: &Palette, label: &str, active: bool) -> bool {
    let font = if active {
        theme::medium(12.0)
    } else {
        FontId::proportional(12.0)
    };
    let measure = theme::medium(12.0);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), measure, Color32::PLACEHOLDER);
    let width = galley.size().x + 22.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 34.0), Sense::click());
    ui.painter().text(
        rect.center() + Vec2::new(0.0, 0.5),
        Align2::CENTER_CENTER,
        label,
        font,
        if active {
            p.text
        } else if resp.hovered() {
            p.text
        } else {
            p.muted
        },
    );
    if active {
        let line = egui::Rect::from_min_size(
            pos2(rect.left() + 9.0, rect.bottom() - 2.0),
            vec2(rect.width() - 18.0, 2.0),
        );
        ui.painter()
            .rect_filled(line, CornerRadius::same(1), p.accent);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn small_tab(ui: &mut Ui, p: &Palette, label: &str, active: bool) -> bool {
    let font = if active {
        theme::medium(11.5)
    } else {
        FontId::proportional(11.5)
    };
    let measure = theme::medium(11.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), measure, Color32::PLACEHOLDER);
    let width = galley.size().x + 4.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 24.0), Sense::click());
    ui.painter().text(
        rect.center() + Vec2::new(0.0, 0.5),
        Align2::CENTER_CENTER,
        label,
        font,
        if active || resp.hovered() {
            p.text
        } else {
            p.muted
        },
    );
    if active {
        let line = egui::Rect::from_min_size(
            pos2(rect.left(), rect.bottom() - 2.0),
            vec2(rect.width(), 2.0),
        );
        ui.painter()
            .rect_filled(line, CornerRadius::same(1), p.accent);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn text_field(
    ui: &mut Ui,
    p: &Palette,
    label: &str,
    value: &mut String,
    hint: &str,
    defined: &std::collections::HashSet<String>,
    changed: &mut bool,
) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [110.0, 26.0],
            egui::Label::new(RichText::new(label).size(12.0).color(p.muted)),
        );
        let id = egui::Id::new(("text_field", label));
        if widgets::var_singleline(
            ui,
            p,
            defined,
            value,
            FontId::monospace(11.5),
            hint,
            [320.0, 26.0],
            true,
            Margin::symmetric(6, 4),
            id,
        )
        .changed()
        {
            *changed = true;
        }
    });
    ui.add_space(4.0);
}

fn secret_field(
    ui: &mut Ui,
    p: &Palette,
    label: &str,
    value: &mut String,
    hint: &str,
    changed: &mut bool,
) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [110.0, 26.0],
            egui::Label::new(RichText::new(label).size(12.0).color(p.muted)),
        );
        if ui
            .add_sized(
                [320.0, 26.0],
                TextEdit::singleline(value)
                    .hint_text(hint)
                    .font(FontId::monospace(11.5))
                    .margin(Margin::symmetric(6, 4))
                    .password(true),
            )
            .changed()
        {
            *changed = true;
        }
        ui.label(
            RichText::new("supports {{variables}}")
                .size(10.0)
                .color(p.faint),
        );
    });
    ui.add_space(4.0);
}

fn response_body(stored: &Option<Arc<StoredResponse>>, example: &str) -> String {
    match stored {
        Some(stored) => match &stored.data {
            Ok(data) => data.body.clone(),
            Err(_) => String::new(),
        },
        None => example.to_string(),
    }
}

fn response_cookies(stored: &Option<Arc<StoredResponse>>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(stored) = stored {
        if let Ok(data) = &stored.data {
            for (key, value) in &data.headers {
                if key.eq_ignore_ascii_case("set-cookie") {
                    let mut parts = value.split(';');
                    let pair = parts.next().unwrap_or("");
                    let mut kv = pair.splitn(2, '=');
                    let name = kv.next().unwrap_or("").trim().to_string();
                    let val = kv.next().unwrap_or("").trim().to_string();
                    let mut extras = Vec::new();
                    for attr in parts {
                        let attr = attr.trim();
                        if !attr.is_empty() {
                            extras.push(attr.to_string());
                        }
                    }
                    let detail = if extras.is_empty() {
                        val
                    } else {
                        format!("{val}  ({})", extras.join("; "))
                    };
                    out.push((name, detail));
                }
            }
        }
    }
    out
}

fn assertion_result_row(ui: &mut Ui, p: &Palette, label: &str, state: Option<bool>, detail: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
        match state {
            Some(true) => {
                ui.painter()
                    .circle_filled(rect.center(), 7.0, p.success_soft);
                widgets::paint_icon(ui.painter(), rect.shrink(3.5), Icon::Check, p.success, 1.4);
            }
            Some(false) => {
                ui.painter()
                    .circle_filled(rect.center(), 7.0, p.danger_soft);
                widgets::paint_icon(ui.painter(), rect.shrink(4.0), Icon::Close, p.danger, 1.4);
            }
            None => {
                ui.painter()
                    .circle_stroke(rect.center(), 6.0, Stroke::new(1.0_f32, p.faint));
            }
        }
        ui.label(RichText::new(label).size(11.5).color(p.text));
        ui.label(RichText::new(detail).size(10.5).color(p.faint));
    });
}
