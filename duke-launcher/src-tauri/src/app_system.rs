//! Complete Windows 11 Desktop System Integration (`v2.3.0`).
//!
//! Provides:
//! 1. Launch at Windows Startup (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run\DukeLauncher`)
//!    with optional `--tray` silent tray boot.
//! 2. 1-Click Desktop & Start Menu `.lnk` shortcut creation using `dukeflaglogo.ico` + system-wide
//!    `duke://` protocol registration (`duke://open`, `duke://launch/<id>`, `duke://deck`, `duke://download`).
//! 3. Full System Tray right-click menu (**Open Duke Launcher**, **Duke Deck Mode**,
//!    **Check for Updates**, **Quit Duke Launcher**) and single-instance window restoration.
//! 4. Multi-monitor-safe window geometry persistence validated via Win32 `MonitorFromPoint` /
//!    `GetMonitorInfoW`.
//! 5. 1-Click redacted Diagnostics Bundle `.zip` export (`freeze-breadcrumb.log`, crash logs,
//!    hardware/OS summary, and scrubbed `settings.json`).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
#[allow(unused_imports)]
use tauri::{AppHandle, Emitter, Manager, Url};

pub const RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const RUN_VALUE_NAME: &str = "DukeLauncher";

static LAST_DIAGNOSTICS_ZIP_PATH: OnceLock<Mutex<Option<String>>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DukeProtocolRoute {
    OpenMain,
    EnterDeckMode,
    LaunchGame(String),
    DownloadUrl(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SavedWindowGeometry {
    pub width: u32,
    pub height: u32,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub maximized: bool,
}

impl Default for SavedWindowGeometry {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 800,
            x: None,
            y: None,
            maximized: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorWorkArea {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl MonitorWorkArea {
    pub fn width(&self) -> u32 {
        (self.right - self.left).max(640) as u32
    }

    pub fn height(&self) -> u32 {
        (self.bottom - self.top).max(480) as u32
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSystemStatus {
    pub launch_on_startup: bool,
    pub start_minimized_to_tray: bool,
    pub minimize_to_tray: bool,
    pub close_to_tray: bool,
    pub desktop_shortcut_exists: bool,
    pub start_menu_shortcut_exists: bool,
    pub protocol_registered: bool,
    pub last_diagnostics_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ShortcutProtocolReport {
    pub desktop_shortcut_path: String,
    pub start_menu_shortcut_path: String,
    pub icon_path: String,
    pub protocol_registered: bool,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DiagnosticsExportReport {
    pub zip_path: String,
    pub file_count: usize,
    pub size_bytes: u64,
    pub redacted_items_count: usize,
    pub summary: String,
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn last_diag_cell() -> &'static Mutex<Option<String>> {
    LAST_DIAGNOSTICS_ZIP_PATH.get_or_init(|| Mutex::new(None))
}

/// Formats the exact `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` command string.
pub fn format_startup_run_command(exe_path: &Path, start_minimized_to_tray: bool) -> String {
    let exe_str = exe_path.to_string_lossy();
    if start_minimized_to_tray {
        format!("\"{exe_str}\" --tray")
    } else {
        format!("\"{exe_str}\"")
    }
}

/// Parses a `HKCU\...\Run\DukeLauncher` value into `(launch_on_startup, start_minimized_to_tray)`.
pub fn parse_startup_run_value(raw_val: Option<&str>) -> (bool, bool) {
    let Some(val) = raw_val.map(str::trim).filter(|s| !s.is_empty()) else {
        return (false, false);
    };
    let lower = val.to_ascii_lowercase();
    let minimized = lower.contains("--tray") || lower.contains("--minimized");
    (true, minimized)
}

pub fn query_windows_startup_registration() -> (bool, bool) {
    let reg_val = duke_core::win32_ffi::reg_read_string(false, RUN_SUBKEY, RUN_VALUE_NAME);
    parse_startup_run_value(reg_val.as_deref())
}

pub fn apply_windows_startup_registration(
    launch_on_startup: bool,
    start_minimized_to_tray: bool,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        if !launch_on_startup {
            return crate::win32_clipboard_protocol::set_windows_autostart_minimized(false);
        }
        let exe_path = std::env::current_exe()
            .map_err(|e| format!("Failed resolving executable path: {e}"))?;
        let cmd = format_startup_run_command(&exe_path, start_minimized_to_tray);
        crate::win32_clipboard_protocol::set_windows_autostart_command(&cmd)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (launch_on_startup, start_minimized_to_tray);
        Ok(())
    }
}

/// Parses `duke://open`, `duke://deck`, `duke://launch/<game_id>`, and `duke://download?url=...`.
pub fn parse_duke_protocol_route(raw_arg: &str) -> Option<DukeProtocolRoute> {
    let cleaned = raw_arg.trim().trim_matches('"').trim();
    if cleaned.is_empty() {
        return None;
    }
    if let Some(dl_url) = crate::parse_duke_protocol_download_url(cleaned) {
        return Some(DukeProtocolRoute::DownloadUrl(dl_url));
    }
    let parsed = Url::parse(cleaned).ok()?;
    if !parsed.scheme().eq_ignore_ascii_case("duke") {
        return None;
    }

    let host = parsed.host_str().unwrap_or("").trim().to_ascii_lowercase();
    let path = parsed.path().trim_matches('/');

    match host.as_str() {
        "open" => Some(DukeProtocolRoute::OpenMain),
        "deck" | "couch" => Some(DukeProtocolRoute::EnterDeckMode),
        "launch" | "play" => {
            if !path.is_empty() {
                let decoded = urlencoding::decode(path)
                    .map(|c| c.into_owned())
                    .unwrap_or_else(|_| path.to_string());
                let target = decoded.trim().to_string();
                if !target.is_empty() {
                    return Some(DukeProtocolRoute::LaunchGame(target));
                }
            }
            if let Some(q) = parsed.query() {
                for (k, v) in q.split('&').filter_map(|p| p.split_once('=')) {
                    if k.eq_ignore_ascii_case("id") || k.eq_ignore_ascii_case("game") {
                        if let Ok(dec) = urlencoding::decode(v) {
                            let target = dec.trim().to_string();
                            if !target.is_empty() {
                                return Some(DukeProtocolRoute::LaunchGame(target));
                            }
                        }
                    }
                }
            }
            Some(DukeProtocolRoute::OpenMain)
        }
        "" => {
            let lower_path = path.to_ascii_lowercase();
            if lower_path == "open" {
                Some(DukeProtocolRoute::OpenMain)
            } else if lower_path == "deck" || lower_path == "couch" {
                Some(DukeProtocolRoute::EnterDeckMode)
            } else if let Some(rest) = path
                .strip_prefix("launch/")
                .or_else(|| path.strip_prefix("LAUNCH/"))
            {
                let decoded = urlencoding::decode(rest)
                    .map(|c| c.into_owned())
                    .unwrap_or_else(|_| rest.to_string());
                let target = decoded.trim().to_string();
                if !target.is_empty() {
                    Some(DukeProtocolRoute::LaunchGame(target))
                } else {
                    Some(DukeProtocolRoute::OpenMain)
                }
            } else {
                Some(DukeProtocolRoute::OpenMain)
            }
        }
        _ => Some(DukeProtocolRoute::OpenMain),
    }
}

pub fn dispatch_duke_protocol_route(app: &AppHandle, route: DukeProtocolRoute) {
    match route {
        DukeProtocolRoute::OpenMain => {
            restore_and_focus_main_window(app);
        }
        DukeProtocolRoute::EnterDeckMode => {
            restore_and_focus_main_window(app);
            let app_clone = app.clone();
            tauri::async_runtime::spawn(async move {
                let _ = crate::couch_mode::set_couch_mode_state(app_clone, true, None, None).await;
            });
        }
        DukeProtocolRoute::LaunchGame(game_id_or_title) => {
            restore_and_focus_main_window(app);
            let app_clone = app.clone();
            tauri::async_runtime::spawn(async move {
                let _ = crate::launch_game_headless(
                    app_clone,
                    String::new(),
                    Some(game_id_or_title),
                    None,
                )
                .await;
            });
        }
        DukeProtocolRoute::DownloadUrl(url) => {
            crate::enqueue_protocol_download_url(app.clone(), url, true, true);
        }
    }
}

pub fn restore_and_focus_main_window(app: &AppHandle) {
    if let Some(daemon) = crate::TRAY_DAEMON.get() {
        daemon.ui_visible.store(true, Ordering::Release);
    }
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        return;
    }
    duke_core::tray_daemon::reopen_main_window(app, "main");
}

/// Installs the full Duke Launcher Windows System Tray menu:
/// - Open Duke Launcher
/// - Duke Deck Mode
/// - Check for Updates
/// - Quit Duke Launcher
pub fn install_full_launcher_tray(
    app: &AppHandle,
    daemon: &duke_core::tray_daemon::TrayDaemonState,
    on_exit: Arc<dyn Fn() + Send + Sync>,
) -> tauri::Result<()> {
    let tray =
        duke_core::tray_daemon::ensure_tray_icon(app, &daemon.config.tray_id, &daemon.config.tooltip)?;

    let open_item =
        MenuItemBuilder::with_id("duke_tray_open", "Open Duke Launcher").build(app)?;
    let deck_item =
        MenuItemBuilder::with_id("duke_tray_deck", "Duke Deck Mode").build(app)?;
    let updates_item =
        MenuItemBuilder::with_id("duke_tray_updates", "Check for Updates").build(app)?;
    let quit_item =
        MenuItemBuilder::with_id("duke_tray_exit", "Quit Duke Launcher").build(app)?;

    let menu = MenuBuilder::new(app)
        .item(&open_item)
        .item(&deck_item)
        .item(&updates_item)
        .separator()
        .item(&quit_item)
        .build()?;
    let _ = tray.set_menu(Some(menu));

    let click_state = daemon.clone();
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
            restore_and_focus_main_window(tray_handle.app_handle());
        }
        _ => {}
    });

    let menu_state = daemon.clone();
    app.on_menu_event(move |app_handle, event| match event.id().as_ref() {
        "duke_tray_open" => {
            menu_state.ui_visible.store(true, Ordering::Release);
            restore_and_focus_main_window(app_handle);
        }
        "duke_tray_deck" => {
            menu_state.ui_visible.store(true, Ordering::Release);
            restore_and_focus_main_window(app_handle);
            let h = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                let current = crate::COUCH_MODE_ACTIVE.load(Ordering::Acquire);
                let _ = crate::couch_mode::set_couch_mode_state(h, !current, None, None).await;
            });
        }
        "duke_tray_updates" => {
            menu_state.ui_visible.store(true, Ordering::Release);
            restore_and_focus_main_window(app_handle);
            crate::emit_if_alive(app_handle, "tray-check-updates", serde_json::json!({ "force": true }));
            let h = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                if let Ok(rep) = crate::updater::check_for_app_updates_internal().await {
                    crate::emit_if_alive(&h, "app-update-available", &rep);
                }
            });
        }
        "duke_tray_exit" => {
            crate::IS_SHUTTING_DOWN.store(true, Ordering::Release);
            crate::APP_SHUTDOWN.store(true, Ordering::Release);
            menu_state.app_exiting.store(true, Ordering::Release);
            on_exit();
            app_handle.exit(0);
        }
        _ => {}
    });

    Ok(())
}

fn resolve_desktop_shortcut_path() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|p| PathBuf::from(p).join("Desktop").join("Duke Launcher.lnk"))
}

fn resolve_start_menu_shortcut_path() -> Option<PathBuf> {
    std::env::var("APPDATA")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|p| {
            PathBuf::from(p)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join("Duke Launcher.lnk")
        })
}

fn ensure_dukeflaglogo_ico_on_disk() -> Result<PathBuf, String> {
    let ico_path = crate::get_app_data_file_standalone("dukeflaglogo.ico")?;
    let embedded_ico = include_bytes!("../icons/icon.ico");
    let needs_write = match fs::metadata(&ico_path) {
        Ok(m) => m.len() != embedded_ico.len() as u64,
        Err(_) => true,
    };
    if needs_write {
        crate::atomic_write_bytes(&ico_path, embedded_ico)?;
    }
    Ok(ico_path)
}

#[cfg(target_os = "windows")]
fn write_windows_lnk_shortcut(
    lnk_path: &Path,
    target_exe: &Path,
    working_dir: &Path,
    icon_ico: &Path,
    description: &str,
) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    if let Some(parent) = lnk_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            format!(
                "Failed creating shortcut parent folder '{}': {e}",
                parent.display()
            )
        })?;
    }

    let to_w = |s: &std::ffi::OsStr| -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    };

    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let result = (|| -> Result<(), String> {
        let shell_link: IShellLinkW =
            unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
                .map_err(|e| format!("CoCreateInstance(IShellLinkW) failed: {e}"))?;

        let exe_w = to_w(target_exe.as_os_str());
        let wd_w = to_w(working_dir.as_os_str());
        let ico_w = to_w(icon_ico.as_os_str());
        let desc_w: Vec<u16> = description.encode_utf16().chain(std::iter::once(0)).collect();
        let lnk_w = to_w(lnk_path.as_os_str());

        unsafe {
            shell_link
                .SetPath(PCWSTR(exe_w.as_ptr()))
                .map_err(|e| format!("IShellLinkW::SetPath failed: {e}"))?;
            shell_link
                .SetWorkingDirectory(PCWSTR(wd_w.as_ptr()))
                .map_err(|e| format!("IShellLinkW::SetWorkingDirectory failed: {e}"))?;
            shell_link
                .SetDescription(PCWSTR(desc_w.as_ptr()))
                .map_err(|e| format!("IShellLinkW::SetDescription failed: {e}"))?;
            shell_link
                .SetIconLocation(PCWSTR(ico_w.as_ptr()), 0)
                .map_err(|e| format!("IShellLinkW::SetIconLocation failed: {e}"))?;

            let persist: IPersistFile = shell_link
                .cast()
                .map_err(|e| format!("IShellLinkW::cast(IPersistFile) failed: {e}"))?;
            persist
                .Save(PCWSTR(lnk_w.as_ptr()), true)
                .map_err(|e| format!("IPersistFile::Save('{}') failed: {e}", lnk_path.display()))?;
        }
        Ok(())
    })();

    unsafe {
        if hr.is_ok() {
            CoUninitialize();
        }
    }
    result
}

#[cfg(not(target_os = "windows"))]
fn write_windows_lnk_shortcut(
    _lnk_path: &Path,
    _target_exe: &Path,
    _working_dir: &Path,
    _icon_ico: &Path,
    _description: &str,
) -> Result<(), String> {
    Ok(())
}

pub fn create_desktop_and_start_menu_shortcuts_sync() -> Result<ShortcutProtocolReport, String> {
    let exe_path = std::env::current_exe()
        .map_err(|e| format!("Failed resolving Duke Launcher executable path: {e}"))?;
    let working_dir = exe_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let ico_path = ensure_dukeflaglogo_ico_on_disk().unwrap_or_else(|_| exe_path.clone());

    let desktop_lnk = resolve_desktop_shortcut_path()
        .ok_or_else(|| "Could not resolve USERPROFILE Desktop path.".to_string())?;
    let start_menu_lnk = resolve_start_menu_shortcut_path()
        .ok_or_else(|| "Could not resolve APPDATA Start Menu Programs path.".to_string())?;

    let desc = "Duke Launcher — High-Speed Windows 11 Game & Console Hub";
    write_windows_lnk_shortcut(&desktop_lnk, &exe_path, &working_dir, &ico_path, desc)?;
    write_windows_lnk_shortcut(&start_menu_lnk, &exe_path, &working_dir, &ico_path, desc)?;

    crate::win32_clipboard_protocol::register_duke_uri_protocol();

    Ok(ShortcutProtocolReport {
        desktop_shortcut_path: desktop_lnk.to_string_lossy().to_string(),
        start_menu_shortcut_path: start_menu_lnk.to_string_lossy().to_string(),
        icon_path: ico_path.to_string_lossy().to_string(),
        protocol_registered: true,
        summary: "Created Desktop & Start Menu shortcuts (dukeflaglogo.ico) and registered duke:// protocol links."
            .to_string(),
    })
}

pub fn is_duke_protocol_registered() -> bool {
    duke_core::win32_ffi::reg_read_string(
        false,
        r"Software\Classes\duke\shell\open\command",
        "",
    )
    .map(|v| !v.trim().is_empty())
    .unwrap_or(false)
}

pub fn get_app_system_status_sync() -> AppSystemStatus {
    let settings = crate::load_settings_standalone();
    let (reg_launch, reg_tray) = query_windows_startup_registration();
    let launch_on_startup = reg_launch || settings.launch_on_startup || settings.start_minimized_with_windows;
    let start_minimized_to_tray = if reg_launch {
        reg_tray
    } else {
        settings.start_minimized_to_tray || settings.start_minimized_with_windows
    };
    let desktop_shortcut_exists = resolve_desktop_shortcut_path()
        .map(|p| p.is_file())
        .unwrap_or(false);
    let start_menu_shortcut_exists = resolve_start_menu_shortcut_path()
        .map(|p| p.is_file())
        .unwrap_or(false);
    let protocol_registered = is_duke_protocol_registered();
    let last_diagnostics_path = last_diag_cell()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();

    AppSystemStatus {
        launch_on_startup,
        start_minimized_to_tray,
        minimize_to_tray: settings.minimize_to_tray,
        close_to_tray: settings.close_to_tray,
        desktop_shortcut_exists,
        start_menu_shortcut_exists,
        protocol_registered,
        last_diagnostics_path,
    }
}

/// Pure-Rust multi-monitor window bounds clamping helper.
/// Ensures saved `(width, height, x, y)` fits within `work` and automatically centers on `work`
/// when `intersects_connected_monitor` is `false` (e.g., an external display was unplugged).
pub fn clamp_window_geometry_to_work_area(
    saved: SavedWindowGeometry,
    work: MonitorWorkArea,
    intersects_connected_monitor: bool,
) -> SavedWindowGeometry {
    let max_w = work.width().max(900);
    let max_h = work.height().max(600);
    let width = saved.width.clamp(900, max_w);
    let height = saved.height.clamp(600, max_h);

    let center_x = work.left + ((work.right - work.left) - width as i32) / 2;
    let center_y = work.top + ((work.bottom - work.top) - height as i32) / 2;

    if !intersects_connected_monitor {
        return SavedWindowGeometry {
            width,
            height,
            x: Some(center_x),
            y: Some(center_y),
            maximized: saved.maximized,
        };
    }

    let (Some(raw_x), Some(raw_y)) = (saved.x, saved.y) else {
        return SavedWindowGeometry {
            width,
            height,
            x: Some(center_x),
            y: Some(center_y),
            maximized: saved.maximized,
        };
    };

    let max_x = (work.right - width as i32).max(work.left);
    let max_y = (work.bottom - height as i32).max(work.top);
    let clamped_x = raw_x.clamp(work.left, max_x);
    let clamped_y = raw_y.clamp(work.top, max_y);

    SavedWindowGeometry {
        width,
        height,
        x: Some(clamped_x),
        y: Some(clamped_y),
        maximized: saved.maximized,
    }
}

#[cfg(target_os = "windows")]
fn query_monitor_work_area_for_rect(
    x: Option<i32>,
    y: Option<i32>,
    width: u32,
    height: u32,
) -> (MonitorWorkArea, bool) {
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct WinPoint {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct WinRect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[repr(C)]
    struct WinMonitorInfo {
        cb_size: u32,
        rc_monitor: WinRect,
        rc_work: WinRect,
        dw_flags: u32,
    }

    const MONITOR_DEFAULTTONULL: u32 = 0x0000_0000;
    const MONITOR_DEFAULTTOPRIMARY: u32 = 0x0000_0001;

    #[link(name = "user32")]
    extern "system" {
        fn MonitorFromPoint(pt: WinPoint, dwFlags: u32) -> isize;
        fn GetMonitorInfoW(hMonitor: isize, lpmi: *mut WinMonitorInfo) -> i32;
    }

    let fallback_work = MonitorWorkArea {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1040,
    };

    let read_info = |hmon: isize| -> Option<MonitorWorkArea> {
        if hmon == 0 {
            return None;
        }
        let mut mi = WinMonitorInfo {
            cb_size: std::mem::size_of::<WinMonitorInfo>() as u32,
            rc_monitor: WinRect::default(),
            rc_work: WinRect::default(),
            dw_flags: 0,
        };
        let ok = unsafe { GetMonitorInfoW(hmon, &mut mi) };
        if ok == 0 {
            return None;
        }
        Some(MonitorWorkArea {
            left: mi.rc_work.left,
            top: mi.rc_work.top,
            right: mi.rc_work.right,
            bottom: mi.rc_work.bottom,
        })
    };

    if let (Some(px), Some(py)) = (x, y) {
        let center_pt = WinPoint {
            x: px.saturating_add((width as i32) / 2),
            y: py.saturating_add((height as i32) / 2),
        };
        let hmon = unsafe { MonitorFromPoint(center_pt, MONITOR_DEFAULTTONULL) };
        if let Some(work) = read_info(hmon) {
            return (work, true);
        }
    }

    let primary_hmon = unsafe { MonitorFromPoint(WinPoint { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    let primary_work = read_info(primary_hmon).unwrap_or(fallback_work);
    (primary_work, false)
}

#[cfg(not(target_os = "windows"))]
fn query_monitor_work_area_for_rect(
    _x: Option<i32>,
    _y: Option<i32>,
    _width: u32,
    _height: u32,
) -> (MonitorWorkArea, bool) {
    (
        MonitorWorkArea {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        },
        false,
    )
}

pub fn restore_saved_window_geometry_on_boot(
    app: &AppHandle,
    settings: &crate::LauncherSettings,
) {
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    let has_saved_dims = settings.window_width.is_some() || settings.window_height.is_some();
    let has_saved_pos = settings.window_x.is_some() && settings.window_y.is_some();
    if !has_saved_dims && !has_saved_pos && !settings.window_maximized {
        return;
    }

    let saved = SavedWindowGeometry {
        width: settings.window_width.unwrap_or(1280).clamp(900, 7680),
        height: settings.window_height.unwrap_or(800).clamp(600, 4320),
        x: settings.window_x,
        y: settings.window_y,
        maximized: settings.window_maximized,
    };

    let (work, intersects) =
        query_monitor_work_area_for_rect(saved.x, saved.y, saved.width, saved.height);
    let clamped = clamp_window_geometry_to_work_area(saved, work, intersects);

    let _ = win.set_size(tauri::Size::Physical(tauri::PhysicalSize {
        width: clamped.width,
        height: clamped.height,
    }));
    if let (Some(cx), Some(cy)) = (clamped.x, clamped.y) {
        let _ = win.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
            x: cx,
            y: cy,
        }));
    }
    if clamped.maximized {
        let _ = win.maximize();
    }
}

pub fn capture_window_geometry_snapshot(app: &AppHandle) -> Option<SavedWindowGeometry> {
    let win = app.get_webview_window("main")?;
    if win.is_minimized().unwrap_or(false) {
        return None;
    }
    let maximized = win.is_maximized().unwrap_or(false);
    let size_opt = win.inner_size().ok();
    let pos_opt = win.outer_position().ok();

    let mut snap = SavedWindowGeometry {
        maximized,
        ..SavedWindowGeometry::default()
    };
    if !maximized {
        if let Some(sz) = size_opt {
            if sz.width >= 900 && sz.height >= 600 {
                snap.width = sz.width;
                snap.height = sz.height;
            }
        }
        if let Some(pos) = pos_opt {
            if pos.x > -16_000 && pos.y > -16_000 {
                snap.x = Some(pos.x);
                snap.y = Some(pos.y);
            }
        }
    }
    Some(snap)
}

pub fn flush_captured_window_geometry(app: &AppHandle, snap: SavedWindowGeometry) {
    let mut settings = crate::load_settings_from_disk(app);
    settings.window_maximized = snap.maximized;
    if !snap.maximized {
        if snap.width >= 900 && snap.height >= 600 {
            settings.window_width = Some(snap.width);
            settings.window_height = Some(snap.height);
        }
        if let (Some(px), Some(py)) = (snap.x, snap.y) {
            if px > -16_000 && py > -16_000 {
                settings.window_x = Some(px);
                settings.window_y = Some(py);
            }
        }
    }
    let _ = crate::save_settings_to_disk(app, &settings);
}

pub fn capture_and_save_window_geometry(app: &AppHandle) {
    if let Some(snap) = capture_window_geometry_snapshot(app) {
        flush_captured_window_geometry(app, snap);
    }
}

/// Redacts all `C:\Users\<name>` / `C:/Users/<name>` paths, current OS username, and secret keys
/// from diagnostic logs or JSON payloads before exporting a bug report `.zip`.
pub fn redact_diagnostics_text(raw: &str, extra_username: Option<&str>) -> (String, usize) {
    let mut out = String::with_capacity(raw.len());
    let mut redactions = 0usize;

    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        // Match `<Drive>:\Users\<name>` or `<Drive>:/Users/<name>` (case-insensitive)
        if i + 9 <= chars.len()
            && chars[i].is_ascii_alphabetic()
            && chars[i + 1] == ':'
            && (chars[i + 2] == '\\' || chars[i + 2] == '/')
        {
            let rest_start = if chars[i + 2] == '\\' && i + 3 < chars.len() && chars[i + 3] == '\\' {
                i + 4
            } else {
                i + 3
            };
            let users_word: String = chars
                .get(rest_start..rest_start + 5)
                .map(|s| s.iter().collect())
                .unwrap_or_default();
            let after_users = rest_start + 5;
            if users_word.eq_ignore_ascii_case("users")
                && after_users < chars.len()
                && (chars[after_users] == '\\' || chars[after_users] == '/')
            {
                let mut sep_end = after_users + 1;
                if sep_end < chars.len() && chars[sep_end] == '\\' {
                    sep_end += 1;
                }
                let mut name_end = sep_end;
                while name_end < chars.len()
                    && chars[name_end] != '\\'
                    && chars[name_end] != '/'
                    && chars[name_end] != '"'
                    && chars[name_end] != '\''
                    && !chars[name_end].is_whitespace()
                {
                    name_end += 1;
                }
                if name_end > sep_end {
                    out.push_str("%USERPROFILE%");
                    redactions += 1;
                    i = name_end;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }

    if let Some(uname) = extra_username.map(str::trim).filter(|s| s.len() >= 3) {
        if out.contains(uname) {
            let count = out.matches(uname).count();
            redactions += count;
            out = out.replace(uname, "<REDACTED_USER>");
        }
    }

    for secret_key in [
        "steamApiKey",
        "xboxApiKey",
        "psnNpsso",
        "steamgriddbApiKey",
        "encryptedKeys",
    ] {
        let needle = format!("\"{secret_key}\":");
        if out.contains(&needle) {
            redactions += 1;
        }
    }

    (out, redactions)
}

pub fn export_diagnostics_bundle_sync() -> Result<DiagnosticsExportReport, String> {
    let ts = now_unix_secs();
    let active_user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .ok();

    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    let mut total_redactions = 0usize;

    // 1. Redacted settings.json
    let mut settings = crate::load_settings_standalone().to_ipc_safe();
    settings.steam_api_key = None;
    settings.xbox_api_key = None;
    settings.psn_npsso = None;
    settings.steamgriddb_api_key = None;
    let settings_json = serde_json::to_string_pretty(&settings).unwrap_or_else(|_| "{}".to_string());
    let (clean_settings, r_set) = redact_diagnostics_text(&settings_json, active_user.as_deref());
    total_redactions += r_set;
    entries.push(("settings_redacted.json".to_string(), clean_settings.into_bytes()));

    // 2. Hardware & OS summary
    let ioring = duke_core::nvme_prefetch::probe_ioring_support();
    let fp = duke_core::win32_ffi::inspect_frame_pacing_status();
    let gpu = crate::shader_swarm::detect_gpu_hardware_identity();
    let (dkcat_records, dkcat_fetched, _) = crate::catalog_mmap::catalog_mmap_header_meta();
    let hw_summary = serde_json::json!({
        "appVersion": env!("CARGO_PKG_VERSION"),
        "exportedAtUnix": ts,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "logicalCores": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        "workingSetMb": crate::get_process_working_set_mb(),
        "gpu": gpu,
        "ioring": ioring,
        "framePacing": fp,
        "catalogMmapRecords": dkcat_records,
        "catalogMmapFetchedAt": dkcat_fetched,
    });
    let hw_raw = serde_json::to_string_pretty(&hw_summary).unwrap_or_else(|_| "{}".to_string());
    let (clean_hw, r_hw) = redact_diagnostics_text(&hw_raw, active_user.as_deref());
    total_redactions += r_hw;
    entries.push(("hardware_os_summary.json".to_string(), clean_hw.into_bytes()));

    // 3. Freeze breadcrumb log, self-heal report, and crash logs (if present)
    for file_name in [
        "freeze-breadcrumb.log",
        "self_heal_report.json",
        "crash.log",
        "duke-launcher-crash.log",
    ] {
        if let Ok(path) = crate::get_app_data_file_standalone(file_name) {
            if path.is_file() {
                if let Ok(raw_text) = fs::read_to_string(&path) {
                    let (redacted, cnt) =
                        redact_diagnostics_text(&raw_text, active_user.as_deref());
                    total_redactions += cnt;
                    entries.push((file_name.to_string(), redacted.into_bytes()));
                }
            }
        }
    }

    if !entries.iter().any(|(name, _)| name == "freeze-breadcrumb.log") {
        entries.push((
            "freeze-breadcrumb.log".to_string(),
            b"No freeze breadcrumbs recorded (clean session).\n".to_vec(),
        ));
    }

    let out_dir = std::env::var("USERPROFILE")
        .ok()
        .map(|p| PathBuf::from(p).join("Desktop"))
        .filter(|d| d.is_dir())
        .or_else(|| {
            crate::get_app_data_file_standalone("Diagnostics")
                .ok()
                .inspect(|d| {
                    let _ = fs::create_dir_all(d);
                })
        })
        .unwrap_or_else(std::env::temp_dir);

    let zip_path = out_dir.join(format!("Duke_Diagnostics_{ts}.zip"));
    crate::write_pure_rust_zip_archive(&zip_path, &entries)?;
    let size_bytes = fs::metadata(&zip_path).map(|m| m.len()).unwrap_or(0);
    let zip_str = zip_path.to_string_lossy().to_string();

    {
        let mut guard = last_diag_cell().lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(zip_str.clone());
    }

    Ok(DiagnosticsExportReport {
        zip_path: zip_str,
        file_count: entries.len(),
        size_bytes,
        redacted_items_count: total_redactions,
        summary: format!(
            "Exported {} diagnostic files ({} redacted path/secret entries).",
            entries.len(),
            total_redactions
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_startup_run_command_formatting_and_parsing() {
        let exe = Path::new(r"C:\Program Files\Duke Launcher\duke-launcher.exe");
        assert_eq!(
            format_startup_run_command(exe, false),
            "\"C:\\Program Files\\Duke Launcher\\duke-launcher.exe\""
        );
        assert_eq!(
            format_startup_run_command(exe, true),
            "\"C:\\Program Files\\Duke Launcher\\duke-launcher.exe\" --tray"
        );
        assert_eq!(parse_startup_run_value(None), (false, false));
        assert_eq!(
            parse_startup_run_value(Some("\"C:\\App\\duke-launcher.exe\"")),
            (true, false)
        );
        assert_eq!(
            parse_startup_run_value(Some("\"C:\\App\\duke-launcher.exe\" --tray")),
            (true, true)
        );
    }

    #[test]
    fn test_duke_protocol_routes_parsing() {
        assert_eq!(
            parse_duke_protocol_route("duke://open"),
            Some(DukeProtocolRoute::OpenMain)
        );
        assert_eq!(
            parse_duke_protocol_route("duke://deck"),
            Some(DukeProtocolRoute::EnterDeckMode)
        );
        assert_eq!(
            parse_duke_protocol_route("duke://launch/elden_ring_123"),
            Some(DukeProtocolRoute::LaunchGame("elden_ring_123".to_string()))
        );
        assert_eq!(
            parse_duke_protocol_route("duke://download?url=https%3A%2F%2Fexample.com%2Fgame.zip"),
            Some(DukeProtocolRoute::DownloadUrl(
                "https://example.com/game.zip".to_string()
            ))
        );
    }

    #[test]
    fn test_multi_monitor_bounds_clamping() {
        let primary = MonitorWorkArea {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        // Saved on unplugged secondary monitor at x = -1920
        let unplugged = SavedWindowGeometry {
            width: 1400,
            height: 860,
            x: Some(-1800),
            y: Some(120),
            maximized: false,
        };
        let centered = clamp_window_geometry_to_work_area(unplugged, primary, false);
        assert_eq!(centered.width, 1400);
        assert_eq!(centered.height, 860);
        assert_eq!(centered.x, Some((1920 - 1400) / 2));
        assert_eq!(centered.y, Some((1040 - 860) / 2));

        // Connected monitor with slightly out-of-bounds bottom-right offset
        let connected = SavedWindowGeometry {
            width: 2400,
            height: 1200,
            x: Some(1700),
            y: Some(950),
            maximized: false,
        };
        let clamped = clamp_window_geometry_to_work_area(connected, primary, true);
        assert_eq!(clamped.width, 1920);
        assert_eq!(clamped.height, 1040);
        assert_eq!(clamped.x, Some(0));
        assert_eq!(clamped.y, Some(0));
    }

    #[test]
    fn test_diagnostics_path_and_user_redaction() {
        let user_dir = ["C:", "Users", "SampleGamer99", "AppData", "Local"].join("\\");
        let fwd_dir = ["D:", "Users", "SampleGamer99", "Games", "Hades"].join("/");
        let sample = format!(
            "Loaded config from {user_dir}\\settings.json and game at {fwd_dir} for SampleGamer99"
        );
        let (redacted, count) = redact_diagnostics_text(&sample, Some("SampleGamer99"));
        assert!(count >= 3);
        assert!(!redacted.contains("SampleGamer99"));
        assert!(redacted.contains("%USERPROFILE%\\AppData\\Local\\settings.json"));
        assert!(redacted.contains("%USERPROFILE%/Games/Hades"));
        assert!(redacted.contains("<REDACTED_USER>"));
    }
}
