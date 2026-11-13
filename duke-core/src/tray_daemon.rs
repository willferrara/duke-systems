//! Headless Tauri v2 tray-first lifecycle helper.
//!
//! Destroys/closes the WebView2 window on close or minimize-to-tray (reducing Chromium
//! renderer RAM to 0 MB) and trims the Rust working set while keeping the backend engine
//! running in the system tray.

use crate::win32_ffi::trim_current_working_set;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, Runtime, WebviewWindowBuilder, WindowEvent};

#[derive(Clone)]
pub struct TrayDaemonConfig {
    pub tray_id: String,
    pub window_label: String,
    pub tooltip: String,
    pub open_label: String,
    pub trim_label: String,
    pub exit_label: String,
    pub close_to_tray: Arc<AtomicBool>,
    pub minimize_to_tray: Arc<AtomicBool>,
}

impl Default for TrayDaemonConfig {
    fn default() -> Self {
        Self {
            tray_id: "main".to_string(),
            window_label: "main".to_string(),
            tooltip: "Duke Systems".to_string(),
            open_label: "Open Window".to_string(),
            trim_label: "Trim Memory".to_string(),
            exit_label: "Exit".to_string(),
            close_to_tray: Arc::new(AtomicBool::new(true)),
            minimize_to_tray: Arc::new(AtomicBool::new(true)),
        }
    }
}

#[derive(Clone)]
pub struct TrayDaemonState {
    pub config: TrayDaemonConfig,
    pub ui_visible: Arc<AtomicBool>,
    pub app_exiting: Arc<AtomicBool>,
}

impl TrayDaemonState {
    pub fn new(config: TrayDaemonConfig) -> Self {
        Self {
            config,
            ui_visible: Arc::new(AtomicBool::new(true)),
            app_exiting: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// Schedules staged working-set trims after WebView2 window destruction so Windows
/// reclaims Chromium renderer & COM teardown pages immediately.
pub fn schedule_delayed_tray_trims() {
    std::thread::spawn(|| {
        trim_current_working_set();
        std::thread::sleep(Duration::from_millis(160));
        trim_current_working_set();
        std::thread::sleep(Duration::from_millis(650));
        trim_current_working_set();
    });
}

/// Re-focuses an existing WebView2 window or recreates it from `tauri.conf.json` config.
pub fn reopen_main_window<R: Runtime>(app: &AppHandle<R>, window_label: &str) {
    if let Some(win) = app.get_webview_window(window_label) {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        return;
    }

    let handle = app.clone();
    let label = window_label.to_string();
    std::thread::spawn(move || {
        let win_cfg = handle
            .config()
            .app
            .windows
            .iter()
            .find(|w| w.label == label)
            .or_else(|| handle.config().app.windows.first())
            .cloned();
        if let Some(cfg) = win_cfg
            && let Ok(win) = WebviewWindowBuilder::from_config(&handle, &cfg).and_then(|b| b.build())
            {
                let _ = win.show();
                let _ = win.set_focus();
            }
    });
}

/// Closes/destroys the WebView2 window (dropping Chromium renderer RAM to 0 MB)
/// and trims the Rust host working set.
pub fn destroy_window_for_tray<R: Runtime>(app: &AppHandle<R>, window_label: &str) {
    let handle = app.clone();
    let label = window_label.to_string();
    std::thread::spawn(move || {
        if let Some(win) = handle.get_webview_window(&label) {
            let _ = win.destroy();
        }
        schedule_delayed_tray_trims();
    });
}

/// Reuses an existing tray icon (e.g. configured in `tauri.conf.json`) or builds one.
pub fn ensure_tray_icon<R: Runtime>(
    app: &AppHandle<R>,
    tray_id: &str,
    tooltip: &str,
) -> tauri::Result<TrayIcon<R>> {
    if let Some(existing) = app.tray_by_id(tray_id) {
        let _ = existing.set_tooltip(Some(tooltip));
        return Ok(existing);
    }

    let mut builder = TrayIconBuilder::with_id(tray_id)
        .tooltip(tooltip)
        .show_menu_on_left_click(false);
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)
}

/// Installs a standard headless-capable tray icon + menu (`Open`, `Trim Memory`, `Exit`).
pub fn install_standard_tray<R: Runtime>(
    app: &AppHandle<R>,
    daemon: &TrayDaemonState,
    on_show: Option<Arc<dyn Fn() + Send + Sync>>,
    on_exit: Option<Arc<dyn Fn() + Send + Sync>>,
) -> tauri::Result<()> {
    let tray = ensure_tray_icon(app, &daemon.config.tray_id, &daemon.config.tooltip)?;

    let open_item = MenuItemBuilder::with_id("duke_tray_open", &daemon.config.open_label).build(app)?;
    let trim_item = MenuItemBuilder::with_id("duke_tray_trim", &daemon.config.trim_label).build(app)?;
    let exit_item = MenuItemBuilder::with_id("duke_tray_exit", &daemon.config.exit_label).build(app)?;

    let menu = MenuBuilder::new(app)
        .item(&open_item)
        .item(&trim_item)
        .separator()
        .item(&exit_item)
        .build()?;
    let _ = tray.set_menu(Some(menu));

    let click_state = daemon.clone();
    let click_on_show = on_show.clone();
    let click_label = daemon.config.window_label.clone();
    tray.on_tray_icon_event(move |tray_handle, event| match event {
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        }
        | TrayIconEvent::DoubleClick {
            button: MouseButton::Left,
            ..
        } => {
            click_state.ui_visible.store(true, Ordering::Release);
            if let Some(cb) = &click_on_show {
                cb();
            }
            reopen_main_window(tray_handle.app_handle(), &click_label);
        }
        _ => {}
    });

    let menu_state = daemon.clone();
    app.on_menu_event(move |app_handle, event| {
        match event.id().as_ref() {
            "duke_tray_open" => {
                menu_state.ui_visible.store(true, Ordering::Release);
                if let Some(cb) = &on_show {
                    cb();
                }
                reopen_main_window(app_handle, &menu_state.config.window_label);
            }
            "duke_tray_trim" => {
                trim_current_working_set();
            }
            "duke_tray_exit" => {
                menu_state.app_exiting.store(true, Ordering::Release);
                if let Some(cb) = &on_exit {
                    cb();
                }
                app_handle.exit(0);
            }
            _ => {}
        }
    });

    Ok(())
}

/// Processes Tauri `RunEvent` window close/minimize/exit lifecycle for tray-first apps.
pub fn handle_run_event<R: Runtime>(
    app_handle: &AppHandle<R>,
    event: &RunEvent,
    daemon: &TrayDaemonState,
    on_hide: Option<&dyn Fn()>,
    on_exit: Option<&dyn Fn()>,
) {
    match event {
        RunEvent::WindowEvent { label, event, .. } if *label == daemon.config.window_label => {
            match event {
                WindowEvent::CloseRequested { .. } => {
                    if !daemon.config.close_to_tray.load(Ordering::Relaxed) {
                        daemon.app_exiting.store(true, Ordering::Release);
                        if let Some(cb) = on_exit {
                            cb();
                        }
                    } else {
                        daemon.ui_visible.store(false, Ordering::Release);
                        if let Some(cb) = on_hide {
                            cb();
                        }
                        schedule_delayed_tray_trims();
                    }
                }
                WindowEvent::Destroyed => {
                    daemon.ui_visible.store(false, Ordering::Release);
                    if let Some(cb) = on_hide {
                        cb();
                    }
                    schedule_delayed_tray_trims();
                }
                WindowEvent::Resized(_) => {
                    if daemon.config.minimize_to_tray.load(Ordering::Relaxed)
                        && let Some(win) = app_handle.get_webview_window(&daemon.config.window_label)
                            && win.is_minimized().unwrap_or(false) {
                                daemon.ui_visible.store(false, Ordering::Release);
                                if let Some(cb) = on_hide {
                                    cb();
                                }
                                destroy_window_for_tray(app_handle, &daemon.config.window_label);
                            }
                }
                _ => {}
            }
        }
        RunEvent::ExitRequested { api, .. }
            if !daemon.app_exiting.load(Ordering::Acquire) => {
                api.prevent_exit();
            }
        _ => {}
    }
}
