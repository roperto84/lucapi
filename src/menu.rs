//! Native macOS menu bar built with muda.

#[cfg(target_os = "macos")]
use muda::{AboutMetadata, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    NewRequest,
    Save,
    ImportCurl,
    ImportApi,
    Export,
    PublishDocs,
    Settings,
    Send,
    CopyCurl,
    Snippets,
    Undo,
    Redo,
}

pub struct NativeMenu {
    #[cfg(target_os = "macos")]
    _menu: Menu,
}

#[cfg(target_os = "macos")]
pub fn install() -> Option<NativeMenu> {
    let menu = Menu::new();

    let app_menu = Submenu::new("LucAPI", true);
    let about = PredefinedMenuItem::about(
        Some("About LucAPI"),
        Some(AboutMetadata {
            name: Some("LucAPI".to_string()),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            short_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            ..Default::default()
        }),
    );
    let settings = MenuItem::with_id("settings", "Settings…", true, None);
    let hide = PredefinedMenuItem::hide(Some("Hide LucAPI"));
    let hide_others = PredefinedMenuItem::hide_others(Some("Hide Others"));
    let show_all = PredefinedMenuItem::show_all(Some("Show All"));
    let quit = PredefinedMenuItem::quit(Some("Quit LucAPI"));
    let _ = app_menu.append_items(&[
        &about,
        &PredefinedMenuItem::separator(),
        &settings,
        &PredefinedMenuItem::separator(),
        &hide,
        &hide_others,
        &show_all,
        &PredefinedMenuItem::separator(),
        &quit,
    ]);

    let file_menu = Submenu::new("File", true);
    let new_request = MenuItem::with_id("new-request", "New Request", true, None);
    let save = MenuItem::with_id("save", "Save", true, None);
    let import_curl = MenuItem::with_id("import-curl", "Import from cURL…", true, None);
    let import_api = MenuItem::with_id(
        "import-api",
        "Import OpenAPI / Postman…",
        true,
        None,
    );
    let export = MenuItem::with_id("export", "Export…", true, None);
    let publish_docs = MenuItem::with_id(
        "publish-docs",
        "Publish documentation…",
        true,
        None,
    );
    let _ = file_menu.append_items(&[
        &new_request,
        &save,
        &PredefinedMenuItem::separator(),
        &import_curl,
        &import_api,
        &export,
        &publish_docs,
    ]);

    let edit_menu = Submenu::new("Edit", true);
    let undo = MenuItem::with_id("undo", "Undo", true, None);
    let redo = MenuItem::with_id("redo", "Redo", true, None);
    // Cut/Copy/Paste are intentionally left to egui: native items would grab
    // their key equivalents and the egui text widgets would never see them.
    let _ = edit_menu.append_items(&[&undo, &redo]);

    let request_menu = Submenu::new("Request", true);
    let send = MenuItem::with_id("send", "Send", true, None);
    let copy_curl = MenuItem::with_id("copy-curl", "Copy as cURL", true, None);
    let snippets = MenuItem::with_id("snippets", "Code Snippets…", true, None);
    let _ = request_menu.append_items(&[&send, &copy_curl, &snippets]);

    let view_menu = Submenu::new("View", true);
    let settings_view = MenuItem::with_id("settings-view", "Settings…", true, None);
    let _ = view_menu.append(&settings_view);

    let _ = menu.append_items(&[&app_menu, &file_menu, &edit_menu, &request_menu, &view_menu]);
    menu.init_for_nsapp();
    Some(NativeMenu { _menu: menu })
}

#[cfg(not(target_os = "macos"))]
pub fn install() -> Option<NativeMenu> {
    None
}

pub fn poll() -> Option<MenuAction> {
    #[cfg(target_os = "macos")]
    {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let action = match event.id.0.as_str() {
                "new-request" => Some(MenuAction::NewRequest),
                "save" => Some(MenuAction::Save),
                "import-curl" => Some(MenuAction::ImportCurl),
                "import-api" => Some(MenuAction::ImportApi),
                "export" => Some(MenuAction::Export),
                "publish-docs" => Some(MenuAction::PublishDocs),
                "settings" | "settings-view" => Some(MenuAction::Settings),
                "send" => Some(MenuAction::Send),
                "copy-curl" => Some(MenuAction::CopyCurl),
                "snippets" => Some(MenuAction::Snippets),
                "undo" => Some(MenuAction::Undo),
                "redo" => Some(MenuAction::Redo),
                _ => None,
            };
            if action.is_some() {
                return action;
            }
        }
    }
    None
}
