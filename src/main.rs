//! LucAPI — a local-first API design and testing workspace.

mod app;
mod codegen;
mod docs;
mod exporter;
mod history;
mod http;
mod importer;
mod json_tree;
mod menu;
mod oauth;
mod runner;
mod model;
mod theme;
mod timing;
mod vault;
mod vars;
mod widgets;

use std::path::PathBuf;

use app::{LaunchOptions, LucApiApp};
use eframe::egui;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("run") {
        std::process::exit(runner::cli(&args[2..]));
    }
    if args.get(1).map(String::as_str) == Some("secrets") {
        std::process::exit(runner::secrets_cli(&args[2..]));
    }
    if args.get(1).map(String::as_str) == Some("docs") {
        std::process::exit(runner::docs_cli(&args[2..]));
    }
    let mut launch = LaunchOptions {
        screenshot: None,
        view: None,
        dark: false,
        snippets: false,
        import: false,
        import_now: None,
        request: None,
        tab: None,
        api_import: None,
        api_import_run: None,
        export: None,
        url: None,
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--screenshot" => {
                if let Some(path) = args.get(i + 1) {
                    launch.screenshot = Some(PathBuf::from(path));
                    i += 2;
                    continue;
                }
            }
            "--view" => {
                if let Some(view) = args.get(i + 1) {
                    launch.view = Some(view.clone());
                    i += 2;
                    continue;
                }
            }
            "--dark" => {
                launch.dark = true;
            }
            "--snippets" => {
                launch.snippets = true;
            }
            "--import" => {
                launch.import = true;
            }
            "--request" => {
                if let Some(name) = args.get(i + 1) {
                    launch.request = Some(name.clone());
                    i += 2;
                    continue;
                }
            }
            "--tab" => {
                if let Some(tab) = args.get(i + 1) {
                    launch.tab = Some(tab.clone());
                    i += 2;
                    continue;
                }
            }
            "--export" => {
                if let Some(format) = args.get(i + 1) {
                    launch.export = Some(format.clone());
                    i += 2;
                    continue;
                }
            }
            "--api-import" => {
                if let Some(path) = args.get(i + 1) {
                    launch.api_import = Some(path.clone());
                    i += 2;
                    continue;
                }
            }
            "--api-import-run" => {
                if let Some(path) = args.get(i + 1) {
                    launch.api_import_run = Some(path.clone());
                    i += 2;
                    continue;
                }
            }
            other if other.starts_with("http://") || other.starts_with("https://") => {
                launch.url = Some(other.to_string());
            }
            "--import-run" => {
                if let Some(text) = args.get(i + 1) {
                    launch.import_now = Some(text.clone());
                    i += 2;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }

    let icon = image::load_from_memory(include_bytes!("../assets/logo.png"))
        .ok()
        .map(|source| {
            let rgba = source.to_rgba8();
            let (width, height) = rgba.dimensions();
            egui::IconData {
                rgba: rgba.into_raw(),
                width,
                height,
            }
        });

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("LucAPI")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([1000.0, 640.0]);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "LucAPI",
        options,
        Box::new(move |cc| Ok(Box::new(LucApiApp::new(cc, launch)))),
    )
}
