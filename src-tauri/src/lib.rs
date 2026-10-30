mod config;
mod history;
#[cfg(target_os = "macos")]
mod macos_notify;
mod ntfy;

use config::{Config, Server, Subscription};
use history::History;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::async_runtime::JoinHandle;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, State, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

/// Passed by the login item so the app starts minimised to the tray.
const HIDDEN_ARG: &str = "--hidden";

type Task = ((Server, Subscription), JoinHandle<()>);

pub struct AppState {
    config: Mutex<Config>,
    /// subscription id -> (server and subscription it was started with, running task)
    tasks: Mutex<HashMap<String, Task>>,
    history: Mutex<History>,
    statuses: Mutex<HashMap<String, ntfy::Status>>,
}

/// Starts/stops/restarts subscription tasks so they match the current config.
/// Unchanged subscriptions keep their existing connection.
fn sync_subscriptions(app: &AppHandle) {
    let state = app.state::<AppState>();
    let cfg = state.config.lock().unwrap().clone();
    let mut tasks = state.tasks.lock().unwrap();

    let wanted: HashMap<String, (Server, Subscription)> = cfg
        .subscriptions
        .iter()
        .filter(|sub| sub.enabled)
        .filter_map(|sub| {
            let server = cfg.servers.iter().find(|s| s.id == sub.server_id)?;
            Some((sub.id.clone(), (server.clone(), sub.clone())))
        })
        .collect();

    // Stop tasks that were removed, disabled or whose server/subscription settings changed.
    tasks.retain(|id, (running, handle)| {
        let keep = wanted.get(id) == Some(running);
        if !keep {
            handle.abort();
            state.statuses.lock().unwrap().remove(id);
        }
        keep
    });

    for (id, (server, sub)) in wanted {
        if let std::collections::hash_map::Entry::Vacant(slot) = tasks.entry(id) {
            let since = state.history.lock().unwrap().last_id(&sub.id);
            let task = ntfy::run(app.clone(), server.clone(), sub.clone(), since);
            slot.insert(((server, sub), tauri::async_runtime::spawn(task)));
        }
    }
}

#[tauri::command]
fn get_config(state: State<AppState>) -> Config {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
fn save_config(app: AppHandle, state: State<AppState>, config: Config) -> Result<Config, String> {
    let saved = config::save(&app, &config)?;
    *state.config.lock().unwrap() = saved.clone();
    sync_subscriptions(&app);
    Ok(saved)
}

#[tauri::command]
fn get_messages(state: State<AppState>) -> Vec<ntfy::Message> {
    state.history.lock().unwrap().all()
}

#[tauri::command]
fn clear_messages(state: State<AppState>) -> Result<(), String> {
    state.history.lock().unwrap().clear()
}

#[tauri::command]
fn get_statuses(state: State<AppState>) -> Vec<ntfy::Status> {
    state.statuses.lock().unwrap().values().cloned().collect()
}

/// Shows a system notification; `url` is opened when the notification is clicked (macOS).
pub fn notify(app: &AppHandle, id: &str, title: &str, body: &str, url: Option<&str>) {
    #[cfg(target_os = "macos")]
    if macos_notify::is_supported() {
        macos_notify::show(id, title, body, url);
        return;
    }
    let _ = (id, url);
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        eprintln!("failed to show notification: {e}");
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn on_notification_click(app: &AppHandle, url: Option<String>) {
    match url {
        Some(url) if url.starts_with("http://") || url.starts_with("https://") => {
            let _ = app.opener().open_url(url, None::<&str>);
        }
        _ => show_main_window(app),
    }
}

#[tauri::command]
fn test_notification(app: AppHandle) {
    let id = format!("test-{:?}", std::time::SystemTime::now());
    notify(&app, &id, "ntfyui", "Notifications are working 🎉", None);
}

/// "authorized", "denied", "not_determined" or "unknown".
/// Async so the wait for macOS's answer doesn't block the main thread.
#[tauri::command]
async fn notification_permission() -> &'static str {
    #[cfg(target_os = "macos")]
    return macos_notify::permission();
    #[cfg(not(target_os = "macos"))]
    "unknown"
}

/// A message to publish, sent as ntfy's JSON format (supports non-ASCII titles, unlike headers).
#[derive(serde::Deserialize, serde::Serialize)]
struct Outgoing {
    topic: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<u8>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    click: Option<String>,
}

#[tauri::command]
async fn publish(
    state: State<'_, AppState>,
    server_id: String,
    message: Outgoing,
) -> Result<(), String> {
    let server = state
        .config
        .lock()
        .unwrap()
        .servers
        .iter()
        .find(|s| s.id == server_id)
        .cloned()
        .ok_or("unknown server")?;
    let body = serde_json::to_string(&message).map_err(|e| e.to_string())?;
    let url = format!("{}/", server.url.trim_end_matches('/'));
    let mut req = reqwest::Client::new()
        .post(url)
        .header("Content-Type", "application/json")
        .body(body);
    if let Some(h) = ntfy::auth_header(&server) {
        req = req.header("Authorization", h);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {status}: {}", body.trim()));
    }
    Ok(())
}

#[tauri::command]
fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("only http(s) links can be opened".into());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn open_notification_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.Notifications-Settings.extension")
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    let launcher = app.autolaunch();
    if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    }
    .map_err(|e| e.to_string())
}

fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![HIDDEN_ARG]),
        ))
        .setup(|app| {
            let handle = app.handle().clone();
            app.manage(AppState {
                config: Mutex::new(config::load(&handle)),
                tasks: Mutex::new(HashMap::new()),
                history: Mutex::new(History::load(&handle)),
                statuses: Mutex::new(HashMap::new()),
            });

            let show = MenuItem::with_id(app, "show", "Open ntfyui", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            TrayIconBuilder::with_id("tray")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("ntfyui")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            #[cfg(target_os = "macos")]
            macos_notify::init(&handle);
            sync_subscriptions(&handle);
            // When launched at login, stay in the tray instead of opening the window.
            if !std::env::args().any(|a| a == HIDDEN_ARG) {
                show_main_window(&handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window just hides it; the app keeps listening in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            get_messages,
            clear_messages,
            get_statuses,
            test_notification,
            notification_permission,
            open_notification_settings,
            publish,
            open_url,
            get_autostart,
            set_autostart
        ])
        .build(tauri::generate_context!())
        .expect("error while building ntfyui");

    app.run(|_app, _event| {
        // Clicking the Dock icon reopens the hidden window.
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = _event {
            show_main_window(_app);
        }
    });
}
