//! Desktop app (product name: `productName` in `tauri.conf.json`, the single source of the name):
//! a thin Tauri shell around `cawget-core`.
//!
//! The Rust side owns the scheduler (`Manager::run` in daemon mode) and exposes the queue as
//! commands. A 400 ms loop pushes the job list to the window whenever it changed, and engine
//! events (finished/failed) are forwarded as they happen.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use cawget_core::{
    AddRequest, Event, JobView, Manager as Engine, RunOptions, SETTINGS, Settings,
    State as JobState,
};
use cawget_core::{Migration, ProbeInfo};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_opener::OpenerExt;
use tokio_util::sync::CancellationToken;

struct AppState {
    engine: Engine,
    db_path: PathBuf,
    /// What happened to the database of the old location at start (shown in About).
    migration: Migration,
    /// Set if the scheduler could not start (for example another instance holds the queue).
    engine_error: Arc<Mutex<Option<String>>>,
}

#[derive(Serialize)]
struct SegDto {
    start: u64,
    end: u64,
    done: u64,
}

#[derive(Serialize)]
struct JobDto {
    #[serde(flatten)]
    view: JobView,
    /// Saved per-segment progress (for the segmented progress bar).
    parts: Vec<SegDto>,
}

#[derive(Serialize)]
struct SettingInfoDto {
    key: &'static str,
    default: &'static str,
    help: &'static str,
}

#[derive(Serialize)]
struct SettingsDto {
    values: Settings,
    info: Vec<SettingInfoDto>,
    default_dir: String,
}

#[derive(Serialize)]
struct AppInfo {
    version: &'static str,
    db_path: String,
    engine_error: Option<String>,
    /// Set when the queue was copied from the old location at this start.
    migrated_from: Option<String>,
    /// Set when the old queue was in use and is used in place for this run.
    migration_deferred: Option<String>,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn snapshot(engine: &Engine) -> Result<Vec<JobDto>, String> {
    let views = engine.list().map_err(err)?;
    Ok(views
        .into_iter()
        .map(|v| {
            let parts = if matches!(
                v.job.state,
                JobState::Active | JobState::Paused | JobState::Queued
            ) {
                engine
                    .store()
                    .load_segments(v.job.id)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|s| SegDto {
                        start: s.start,
                        end: s.end,
                        done: s.done,
                    })
                    .collect()
            } else {
                Vec::new()
            };
            JobDto { view: v, parts }
        })
        .collect())
}

#[tauri::command]
fn list_jobs(state: State<AppState>) -> Result<Vec<JobDto>, String> {
    snapshot(&state.engine)
}

/// Extra request headers for an optional login (HTTP Basic authorization).
fn auth_headers(login: &Option<String>, password: &Option<String>) -> Vec<String> {
    match login.as_deref().filter(|l| !l.is_empty()) {
        Some(l) => vec![cawget_core::http::basic_auth_header(
            l,
            password.as_deref().unwrap_or(""),
        )],
        None => Vec::new(),
    }
}

/// Asks the server about one address (size, file name, resume support) without adding it.
#[tauri::command]
async fn probe_url(
    state: State<'_, AppState>,
    url: String,
    login: Option<String>,
    password: Option<String>,
) -> Result<ProbeInfo, String> {
    let req = AddRequest {
        url: url.trim().to_string(),
        headers: auth_headers(&login, &password),
        ..Default::default()
    };
    state.engine.probe_url(&req).await
}

/// Adds one job per non-empty line. `output` (a full path) is only allowed for a single address.
/// `paused` = "Start later". `start_in_secs` delays the start.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn add_downloads(
    state: State<AppState>,
    urls: String,
    dir: Option<String>,
    output: Option<String>,
    segments: Option<u32>,
    start_in_secs: Option<i64>,
    paused: Option<bool>,
    login: Option<String>,
    password: Option<String>,
) -> Result<Vec<i64>, String> {
    let not_before = start_in_secs
        .filter(|&s| s > 0)
        .map_or(0, |s| cawget_core::util::unix_now() + s);
    let lines: Vec<&str> = urls
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return Err("Enter at least one http:// or https:// link".into());
    }
    let output = output.filter(|o| !o.trim().is_empty());
    if output.is_some() && lines.len() > 1 {
        return Err("A file name can only be given for a single address".into());
    }
    let headers = auth_headers(&login, &password);
    let mut ids = Vec::new();
    for line in lines {
        let id = state
            .engine
            .add(AddRequest {
                url: line.to_string(),
                dir: dir.clone().filter(|d| !d.trim().is_empty()),
                output: output.clone(),
                segments,
                not_before,
                paused: paused.unwrap_or(false),
                headers: headers.clone(),
                ..Default::default()
            })
            .map_err(err)?;
        ids.push(id);
    }
    Ok(ids)
}

#[tauri::command]
fn pause_job(state: State<AppState>, id: i64) -> Result<(), String> {
    state.engine.pause(id).map_err(err)
}

#[tauri::command]
fn resume_job(state: State<AppState>, id: i64) -> Result<(), String> {
    state.engine.resume(id).map_err(err)
}

#[tauri::command]
fn remove_job(state: State<AppState>, id: i64, delete_file: bool) -> Result<(), String> {
    state.engine.remove(id, delete_file).map_err(err)
}

#[tauri::command]
fn clear_finished(state: State<AppState>) -> Result<usize, String> {
    state.engine.clear_finished().map_err(err)
}

/// Opens a finished download with the desktop's default application.
#[tauri::command]
fn open_file(app: tauri::AppHandle, state: State<AppState>, id: i64) -> Result<(), String> {
    let job = state.engine.get(id).map_err(err)?.job;
    if job.state != JobState::Done {
        return Err("the download is not finished yet".into());
    }
    if !Path::new(&job.out_path).is_file() {
        return Err(format!("file not found: {}", job.out_path));
    }
    app.opener()
        .open_path(job.out_path, None::<&str>)
        .map_err(err)
}

/// Opens the folder that holds (or will hold) the job's file.
#[tauri::command]
fn open_folder(app: tauri::AppHandle, state: State<AppState>, id: i64) -> Result<(), String> {
    let job = state.engine.get(id).map_err(err)?.job;
    let out = job.out_path.as_str();
    let dir: PathBuf = if out.ends_with('/') {
        PathBuf::from(out)
    } else {
        Path::new(out)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| state.engine.settings().effective_download_dir())
    };
    let dir = if dir.is_dir() {
        dir
    } else {
        state.engine.settings().effective_download_dir()
    };
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(err)
}

/// Folder a file of this name goes to when no folder is chosen (its category folder, else the default folder).
#[tauri::command]
fn suggest_folder(state: State<AppState>, file_name: String) -> String {
    state
        .engine
        .settings()
        .folder_for(&file_name)
        .to_string_lossy()
        .into_owned()
}

/// Sets (or with `at` = 0 clears) the start time of downloads. Stopped or failed ones are queued again, so they
/// really start at that time; finished ones are skipped. Returns how many were scheduled.
#[tauri::command]
fn schedule_jobs(state: State<AppState>, ids: Vec<i64>, at: i64) -> Result<usize, String> {
    let mut n = 0;
    for id in ids {
        let job = state.engine.get(id).map_err(err)?.job;
        if job.state == JobState::Done {
            continue;
        }
        state.engine.set_schedule(id, at.max(0)).map_err(err)?;
        if matches!(job.state, JobState::Paused | JobState::Failed) && at > 0 {
            state.engine.resume(id).map_err(err)?;
        }
        n += 1;
    }
    Ok(n)
}

const CLOSE_TO_TRAY_KEY: &str = "ui.close_to_tray";
static CLOSE_TO_TRAY: AtomicBool = AtomicBool::new(false);
static TRAY_OK: AtomicBool = AtomicBool::new(false);

#[derive(Serialize)]
struct TrayState {
    available: bool,
    close_to_tray: bool,
}

/// Whether a tray icon can actually be seen. Building the icon succeeds even when no desktop component displays it,
/// so on Linux this also asks the session bus for a StatusNotifier host. Checked at each use: a panel may start later.
fn tray_visible() -> bool {
    TRAY_OK.load(Ordering::Relaxed) && tray_host_present()
}

#[cfg(target_os = "linux")]
fn tray_host_present() -> bool {
    use gio::glib::{ToVariant, VariantTy};
    let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
        return false;
    };
    bus.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        Some(&("org.kde.StatusNotifierWatcher",).to_variant()),
        Some(VariantTy::new("(b)").expect("valid type")),
        gio::DBusCallFlags::NONE,
        500,
        gio::Cancellable::NONE,
    )
    .ok()
    .and_then(|v| v.get::<(bool,)>())
    .is_some_and(|(owned,)| owned)
}

#[cfg(not(target_os = "linux"))]
fn tray_host_present() -> bool {
    true
}

#[tauri::command]
fn tray_state() -> TrayState {
    TrayState {
        available: tray_visible(),
        close_to_tray: CLOSE_TO_TRAY.load(Ordering::Relaxed),
    }
}

/// The window's close button hides it to the tray instead of quitting (only when a tray icon exists).
#[tauri::command]
fn set_close_to_tray(state: State<AppState>, on: bool) -> Result<(), String> {
    state
        .engine
        .store()
        .set_setting(CLOSE_TO_TRAY_KEY, if on { "1" } else { "0" })
        .map_err(err)?;
    CLOSE_TO_TRAY.store(on, Ordering::Relaxed);
    Ok(())
}

/// Quits for real (also when closing hides to the tray). Running downloads are checkpointed on exit.
#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

const WINDOW_SIZE_KEY: &str = "ui.window_size";
/// Smallest window the layout is designed for (also `minWidth`/`minHeight` in tauri.conf.json).
const MIN_WINDOW: (f64, f64) = (480.0, 360.0);

/// Parses a stored window size ("1180x720", logical px). Sizes below the minimum or above 16k are rejected.
fn parse_window_size(s: &str) -> Option<(f64, f64)> {
    let (w, h) = s.split_once('x')?;
    let (w, h) = (w.trim().parse::<f64>().ok()?, h.trim().parse::<f64>().ok()?);
    let ok = |v: f64, min: f64| v.is_finite() && (min..=16384.0).contains(&v);
    (ok(w, MIN_WINDOW.0) && ok(h, MIN_WINDOW.1)).then_some((w, h))
}

/// Remembers the window's size (not when maximized or fullscreen: those are states, not sizes). The position is
/// not kept: on Wayland and under tiling window managers the compositor decides where windows go.
fn save_window_size(window: &tauri::WebviewWindow) {
    if window.is_maximized().unwrap_or(false) || window.is_fullscreen().unwrap_or(false) {
        return;
    }
    let (Ok(size), Ok(scale)) = (window.inner_size(), window.scale_factor()) else {
        return;
    };
    let l = size.to_logical::<f64>(scale);
    let value = format!("{}x{}", l.width.round(), l.height.round());
    if parse_window_size(&value).is_some() {
        let _ = window
            .state::<AppState>()
            .engine
            .store()
            .set_setting(WINDOW_SIZE_KEY, &value);
    }
}

/// Applies the remembered size, kept within the monitor the window is on.
fn restore_window_size(window: &tauri::WebviewWindow, engine: &Engine) {
    let Some((mut w, mut h)) = engine
        .store()
        .get_setting(WINDOW_SIZE_KEY)
        .ok()
        .flatten()
        .and_then(|s| parse_window_size(&s))
    else {
        return;
    };
    if let Ok(Some(m)) = window.current_monitor() {
        let screen = m.size().to_logical::<f64>(m.scale_factor());
        w = w.min(screen.width).max(MIN_WINDOW.0);
        h = h.min(screen.height).max(MIN_WINDOW.1);
    }
    let _ = window.set_size(tauri::LogicalSize::new(w, h));
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn pause_all(engine: &Engine) {
    if let Ok(list) = engine.list() {
        for v in list {
            if matches!(v.job.state, JobState::Active | JobState::Queued) {
                let _ = engine.pause(v.job.id);
            }
        }
    }
}

fn build_tray(app: &tauri::App, engine: Engine) -> tauri::Result<()> {
    let name = app.package_info().name.clone();
    let show = MenuItem::with_id(app, "show", format!("Show {name}"), true, None::<&str>)?;
    let add = MenuItem::with_id(app, "add", "Add download…", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop_all", "Stop all", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &show,
            &add,
            &PredefinedMenuItem::separator(app)?,
            &stop,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip(&name)
        .menu(&menu)
        .show_menu_on_left_click(false);
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.on_menu_event(move |app, ev| match ev.id().as_ref() {
        "show" => show_main(app),
        "add" => {
            show_main(app);
            let _ = app.emit("tray-add", ());
        }
        "stop_all" => pause_all(&engine),
        "quit" => app.exit(0),
        _ => {}
    })
    .on_tray_icon_event(|tray, ev| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = ev
        {
            let app = tray.app_handle();
            match app.get_webview_window("main") {
                Some(w) if w.is_visible().unwrap_or(false) => {
                    let _ = w.hide();
                }
                _ => show_main(app),
            }
        }
    })
    .build(app)?;
    Ok(())
}

/// True if something exists at `path` (used to warn before an explicit Save as path replaces a file).
#[tauri::command]
fn file_exists(path: String) -> bool {
    Path::new(&path).exists()
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> SettingsDto {
    let values = state.engine.settings();
    let default_dir = values
        .effective_download_dir()
        .to_string_lossy()
        .into_owned();
    SettingsDto {
        values,
        info: SETTINGS
            .iter()
            .map(|s| SettingInfoDto {
                key: s.key,
                default: s.default,
                help: s.help,
            })
            .collect(),
        default_dir,
    }
}

#[tauri::command]
fn set_setting(state: State<AppState>, key: String, value: String) -> Result<(), String> {
    state.engine.set_setting(&key, &value).map_err(err)
}

#[tauri::command]
fn app_info(state: State<AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        db_path: state.db_path.to_string_lossy().into_owned(),
        engine_error: state.engine_error.lock().ok().and_then(|g| g.clone()),
        migrated_from: match &state.migration {
            Migration::Copied { from } => Some(from.to_string_lossy().into_owned()),
            _ => None,
        },
        migration_deferred: match &state.migration {
            Migration::Deferred { old } => Some(old.to_string_lossy().into_owned()),
            _ => None,
        },
    }
}

/// `$CAWGET_DB`, its deprecated aliases (`$JACKDAW_DB`, then `$SEGGET_DB`), or the default location (copying a
/// legacy database there once; the old file is never changed).
fn db_path() -> (PathBuf, Migration) {
    if let Some((p, var)) = cawget_core::paths::db_from_env() {
        if var != cawget_core::paths::DB_ENV {
            eprintln!(
                "note: {var} is deprecated, use {}",
                cawget_core::paths::DB_ENV
            );
        }
        return (p, Migration::NotNeeded);
    }
    cawget_core::paths::resolve_default_db().unwrap_or_else(|e| {
        eprintln!("could not copy the old download queue ({e}); using the new location");
        (
            cawget_core::paths::default_db_in(&cawget_core::paths::data_dir()),
            Migration::NotNeeded,
        )
    })
}

fn main() {
    let context = tauri::generate_context!();
    let product = context
        .config()
        .product_name
        .clone()
        .unwrap_or_else(|| "app".into());
    let (db, migration) = db_path();
    let engine = Engine::open(&db).unwrap_or_else(|e| {
        eprintln!("{product}: cannot open database {}: {e}", db.display());
        std::process::exit(1);
    });
    let shutdown = CancellationToken::new();
    let engine_error = Arc::new(Mutex::new(None));
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            engine: engine.clone(),
            db_path: db,
            migration,
            engine_error: Arc::clone(&engine_error),
        })
        .setup({
            let (engine, shutdown, engine_error) =
                (engine.clone(), shutdown.clone(), Arc::clone(&engine_error));
            move |app| {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.set_title(&app.package_info().name);
                    restore_window_size(&w, &engine);
                }
                CLOSE_TO_TRAY.store(
                    engine
                        .store()
                        .get_setting(CLOSE_TO_TRAY_KEY)
                        .ok()
                        .flatten()
                        .as_deref()
                        == Some("1"),
                    Ordering::Relaxed,
                );
                // The tray needs a system tray library (libayatana-appindicator); without one the app still works.
                match build_tray(app, engine.clone()) {
                    Ok(()) => TRAY_OK.store(true, Ordering::Relaxed),
                    Err(e) => eprintln!("tray icon not available: {e}"),
                }
                // The scheduler: starts queued jobs, applies pause/remove, saves progress on exit.
                let (e, t, slot, handle) = (
                    engine.clone(),
                    shutdown.clone(),
                    engine_error,
                    app.handle().clone(),
                );
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = e
                        .run(
                            RunOptions {
                                only_id: None,
                                daemon: true,
                            },
                            t,
                        )
                        .await
                    {
                        let msg = error.to_string();
                        *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(msg.clone());
                        let _ = handle.emit("engine-error", msg);
                    }
                    let _ = done_tx.send(());
                });

                // Push the job list to the window whenever it changes.
                let (e, handle) = (engine.clone(), app.handle().clone());
                tauri::async_runtime::spawn(async move {
                    let mut last = String::new();
                    let mut tick = tokio::time::interval(Duration::from_millis(400));
                    loop {
                        tick.tick().await;
                        if let Ok(list) = snapshot(&e) {
                            let json = serde_json::to_string(&list).unwrap_or_default();
                            if json != last {
                                last = json;
                                let _ = handle.emit("jobs", &list);
                            }
                        }
                    }
                });

                // Forward engine events (finished / failed / retries) for notifications.
                let (mut rx, handle) = (engine.subscribe(), app.handle().clone());
                tauri::async_runtime::spawn(async move {
                    loop {
                        match rx.recv().await {
                            Ok(ev @ (Event::Done { .. } | Event::Failed { .. })) => {
                                let _ = handle.emit("download-event", &ev);
                            }
                            Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                            Err(_) => break,
                        }
                    }
                });
                Ok(())
            }
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if let Some(w) = window.app_handle().get_webview_window(window.label()) {
                    save_window_size(&w);
                }
                // With "close to tray" on (and a tray icon someone can see), the close button hides the window.
                // Without a visible tray the window closes normally, so it can never become unreachable.
                if CLOSE_TO_TRAY.load(Ordering::Relaxed) && tray_visible() {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_jobs,
            add_downloads,
            probe_url,
            pause_job,
            resume_job,
            remove_job,
            clear_finished,
            open_folder,
            open_file,
            file_exists,
            suggest_folder,
            schedule_jobs,
            tray_state,
            set_close_to_tray,
            quit_app,
            get_settings,
            set_setting,
            app_info
        ])
        .build(context)
        .expect("error while building the main window");

    app.run(move |handle, event| {
        // Quit from the menu or the tray (the window may be hidden in the tray): keep its size too.
        if let RunEvent::ExitRequested { .. } = event
            && let Some(w) = handle.get_webview_window("main")
        {
            save_window_size(&w);
        }
        if let RunEvent::Exit = event {
            // Ask the scheduler to checkpoint running downloads so they resume where they stopped.
            shutdown.cancel();
            let _ = done_rx.recv_timeout(Duration::from_secs(4));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::parse_window_size;

    #[test]
    fn stored_window_sizes_are_validated() {
        assert_eq!(parse_window_size("1180x720"), Some((1180.0, 720.0)));
        assert_eq!(parse_window_size(" 940 x 1000 "), Some((940.0, 1000.0)));
        assert_eq!(parse_window_size("480x360"), Some((480.0, 360.0)));
        for bad in [
            "479x360", "480x359", "x", "", "axb", "1e9x500", "NaNx500", "800",
        ] {
            assert_eq!(parse_window_size(bad), None, "{bad}");
        }
    }
}
