// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use duke_core::pipe_ipc::{DukeBus, DukeEvent, DUKE_PIPE_NAME};
use duke_core::shm_bus::{
    unpack_gpu_telemetry, DukeShmBus, ShmEventType, ShmSourceApp, DUKE_SHM_NAME, SHM_RING_CAPACITY,
};
use duke_core::tray_daemon::{
    destroy_window_for_tray, handle_run_event, install_standard_tray, TrayDaemonConfig,
    TrayDaemonState,
};
use duke_core::win32_ffi::{
    enable_eco_qos_mode, get_current_working_set_mb, get_external_working_set_mb,
    restore_normal_priority_mode, trim_current_working_set, trim_external_pid,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::{AppHandle, Emitter, Manager};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
#[cfg(target_os = "windows")]
use windows::core::BOOL;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM};
#[cfg(target_os = "windows")]
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible,
    SetForegroundWindow, ShowWindow, SW_RESTORE,
};

#[cfg(target_os = "windows")]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

const MAX_ACTIVITY_ITEMS: usize = 120;

static DUKE_BUS: OnceLock<DukeBus> = OnceLock::new();
static DUKE_SHM_BUS: OnceLock<Option<DukeShmBus>> = OnceLock::new();
static SHM_STOP_SIGNAL: AtomicBool = AtomicBool::new(false);
static ACTIVITY_SEQ: AtomicU64 = AtomicU64::new(1);

struct SuiteAppDef {
    id: &'static str,
    name: &'static str,
    subtitle: &'static str,
    folder_name: &'static str,
    product_exe: &'static str,
    cargo_exe: &'static str,
    logo_asset: &'static str,
}

const SUITE_APPS: [SuiteAppDef; 4] = [
    SuiteAppDef {
        id: "launcher",
        name: "Duke Launcher",
        subtitle: "8-Worker Turbo Downloader, Game Library & Crack-Aware Malware Shield",
        folder_name: "duke-launcher",
        product_exe: "Duke Launcher.exe",
        cargo_exe: "duke-launcher.exe",
        logo_asset: "./assets/dukeflag.png",
    },
    SuiteAppDef {
        id: "monitor",
        name: "Duke Monitor",
        subtitle: "Hardware Telemetry, Hybrid P/E-Core EcoQoS & Clean Driver Cockpit",
        folder_name: "duke-monitor",
        product_exe: "Duke Monitor.exe",
        cargo_exe: "duke-monitor.exe",
        logo_asset: "./assets/dukemonitor.png",
    },
    SuiteAppDef {
        id: "capture",
        name: "Duke Capture",
        subtitle: "Zero-Bloat RAM Instant Replay Buffer, Dual WASAPI & Clip Studio",
        folder_name: "duke-capture",
        product_exe: "Duke Capture.exe",
        cargo_exe: "duke-capture.exe",
        logo_asset: "./assets/dukecapture.png",
    },
    SuiteAppDef {
        id: "input",
        name: "Duke Input",
        subtitle: "Sub-10 MB 1000 Hz Controller, Mouse, Gyro-to-Mouse & FPV Remapper",
        folder_name: "duke-input",
        product_exe: "Duke Input.exe",
        cargo_exe: "duke-input.exe",
        logo_asset: "./assets/dukeinput.png",
    },
];

const SUITE_LAUNCH_ORDER: [&str; 4] = ["monitor", "capture", "input", "launcher"];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AutoLaunchConfig {
    pub monitor: bool,
    pub capture: bool,
    pub input: bool,
    pub launcher: bool,
}

impl AutoLaunchConfig {
    fn is_enabled(&self, app_id: &str) -> bool {
        match app_id {
            "monitor" => self.monitor,
            "capture" => self.capture,
            "input" => self.input,
            "launcher" => self.launcher,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuiteSettings {
    pub auto_launch: AutoLaunchConfig,
    pub close_to_tray: bool,
    pub minimize_to_tray: bool,
    pub start_minimized: bool,
    pub autostart_windows: bool,
    pub auto_trim_enabled: bool,
}

impl Default for SuiteSettings {
    fn default() -> Self {
        Self {
            auto_launch: AutoLaunchConfig::default(),
            close_to_tray: true,
            minimize_to_tray: true,
            start_minimized: false,
            autostart_windows: false,
            auto_trim_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuiteAppStatus {
    pub id: String,
    pub name: String,
    pub subtitle: String,
    pub logo_asset: String,
    pub installed: bool,
    pub exe_path: String,
    pub running: bool,
    pub pid: Option<u32>,
    pub ram_mb: f64,
    pub window_open: bool,
    pub status_label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuiteActivityItem {
    pub id: u64,
    pub timestamp_ms: u64,
    pub time_label: String,
    pub event_type: String,
    pub source: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuiteStatusSnapshot {
    pub apps: Vec<SuiteAppStatus>,
    pub total_suite_ram_mb: f64,
    pub apps_ram_mb: f64,
    pub own_ram_mb: f64,
    pub running_count: usize,
    pub installed_count: usize,
    pub pipe_bus_online: bool,
    pub pipe_bus_is_hub: bool,
    pub pipe_name: String,
    pub shm_bus_active: bool,
    pub shm_name: String,
    pub shm_total_frames: u64,
    pub shm_ring_capacity: usize,
    pub shm_active_apps: Vec<String>,
    pub shm_gpu_temp_c: Option<u32>,
    pub shm_gpu_load_pct: Option<u32>,
    pub shm_sys_ram_used_mb: Option<u64>,
    pub portable_mode: bool,
    pub settings_path: String,
    pub settings: SuiteSettings,
    pub activity_feed: Vec<SuiteActivityItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchSuiteSummary {
    pub launched: Vec<String>,
    pub skipped: Vec<String>,
    pub missing: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RamPurgeSummary {
    pub freed_mb: f64,
    pub processes_trimmed: usize,
    pub own_ram_mb: f64,
    pub message: String,
}

struct SharedState {
    settings: Mutex<SuiteSettings>,
    activity_feed: Mutex<VecDeque<SuiteActivityItem>>,
    ui_visible: AtomicBool,
    shm_gpu_temp_c: AtomicU64,
    shm_gpu_load_pct: AtomicU64,
    shm_sys_ram_used_mb: AtomicU64,
    shm_last_tick_ms: AtomicU64,
    daemon: TrayDaemonState,
}

impl SharedState {
    fn new(settings: SuiteSettings, daemon: TrayDaemonState) -> Self {
        Self {
            settings: Mutex::new(settings),
            activity_feed: Mutex::new(VecDeque::with_capacity(MAX_ACTIVITY_ITEMS)),
            ui_visible: AtomicBool::new(true),
            shm_gpu_temp_c: AtomicU64::new(u64::from(u32::MAX)),
            shm_gpu_load_pct: AtomicU64::new(u64::from(u32::MAX)),
            shm_sys_ram_used_mb: AtomicU64::new(0),
            shm_last_tick_ms: AtomicU64::new(0),
            daemon,
        }
    }
}

fn round_tenths(val: f64) -> f64 {
    ((val * 10.0).round() / 10.0).max(0.0)
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn format_clock_hms(epoch_secs: u64) -> String {
    let day_secs = epoch_secs % 86_400;
    let hours = day_secs / 3_600;
    let mins = (day_secs % 3_600) / 60;
    let secs = day_secs % 60;
    format!("{:02}:{:02}:{:02} UTC", hours, mins, secs)
}

fn push_activity(
    state: &SharedState,
    app: Option<&AppHandle>,
    event_type: impl Into<String>,
    source: impl Into<String>,
    summary: impl Into<String>,
) -> SuiteActivityItem {
    let now_ms = current_unix_ms();
    let item = SuiteActivityItem {
        id: ACTIVITY_SEQ.fetch_add(1, Ordering::Relaxed),
        timestamp_ms: now_ms,
        time_label: format_clock_hms(now_ms / 1000),
        event_type: event_type.into(),
        source: source.into(),
        summary: summary.into(),
    };

    if let Ok(mut feed) = state.activity_feed.lock() {
        feed.push_front(item.clone());
        while feed.len() > MAX_ACTIVITY_ITEMS {
            feed.pop_back();
        }
    }

    if let Some(handle) = app {
        let _ = handle.emit("duke-bus-event", &item);
    }

    item
}

fn resolve_portable_data_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    if dir.join("portable.flag").exists() || dir.join("data").is_dir() {
        Some(dir.join("data"))
    } else {
        None
    }
}

fn get_settings_path(app: &AppHandle) -> PathBuf {
    if let Some(portable_dir) = resolve_portable_data_dir() {
        let _ = fs::create_dir_all(&portable_dir);
        return portable_dir.join("suite-settings.json");
    }
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = fs::create_dir_all(&dir);
        return dir.join("suite-settings.json");
    }
    std::env::temp_dir()
        .join("DukeSystems")
        .join("suite-settings.json")
}

#[cfg(target_os = "windows")]
fn read_windows_autostart_enabled() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(key) = hkcu.open_subkey("Software\\Microsoft\\CurrentVersion\\Run") else {
        return false;
    };
    key.get_value::<String, _>("DukeSystems").is_ok()
}

#[cfg(not(target_os = "windows"))]
fn read_windows_autostart_enabled() -> bool {
    false
}

#[cfg(target_os = "windows")]
fn set_windows_autostart(enabled: bool) -> Result<(), String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey("Software\\Microsoft\\CurrentVersion\\Run")
        .map_err(|e| format!("Failed opening Windows Run key: {}", e))?;

    if enabled {
        let exe = std::env::current_exe()
            .map_err(|e| format!("Could not resolve executable path: {}", e))?;
        let val = format!("\"{}\" --minimized", exe.display());
        key.set_value("DukeSystems", &val)
            .map_err(|e| format!("Failed enabling Windows startup: {}", e))?;
    } else {
        let _ = key.delete_value("DukeSystems");
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn set_windows_autostart(_enabled: bool) -> Result<(), String> {
    Ok(())
}

fn load_settings(app: &AppHandle) -> SuiteSettings {
    let path = get_settings_path(app);
    let mut settings = fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<SuiteSettings>(&raw).ok())
        .unwrap_or_default();
    settings.autostart_windows = read_windows_autostart_enabled();
    let _ = save_settings_to_path(&path, &settings);
    settings
}

fn save_settings(app: &AppHandle, settings: &SuiteSettings) -> Result<(), String> {
    let path = get_settings_path(app);
    save_settings_to_path(&path, settings)
}

fn save_settings_to_path(path: &Path, settings: &SuiteSettings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed creating settings directory: {}", e))?;
    }
    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed serializing settings: {}", e))?;
    fs::write(path, json).map_err(|e| format!("Failed writing suite-settings.json: {}", e))
}

fn find_suite_app_def(app_id: &str) -> Option<&'static SuiteAppDef> {
    SUITE_APPS.iter().find(|d| d.id.eq_ignore_ascii_case(app_id))
}

/// Discovers each suite app dynamically relative to `current_exe()` (checking `.` and
/// workspace/sibling directories such as `..\<app-dir>\`) with zero hardcoded personal paths.
fn discover_app_exe(def: &SuiteAppDef) -> Option<PathBuf> {
    let base_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .or_else(|| std::env::current_dir().ok())?;

    let mut candidates: Vec<PathBuf> = Vec::with_capacity(20);
    candidates.push(base_dir.join(def.product_exe));
    candidates.push(base_dir.join(def.cargo_exe));

    let mut cursor = Some(base_dir.as_path());
    for _ in 0..4 {
        let Some(dir) = cursor else {
            break;
        };
        candidates.push(dir.join(def.folder_name).join(def.product_exe));
        candidates.push(
            dir.join(def.folder_name)
                .join("src-tauri")
                .join("target")
                .join("release")
                .join(def.cargo_exe),
        );
        candidates.push(
            dir.join(def.folder_name)
                .join("src-tauri")
                .join("target")
                .join("debug")
                .join(def.cargo_exe),
        );
        candidates.push(dir.join(def.product_exe));
        cursor = dir.parent();
    }

    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let lad = PathBuf::from(local_app_data).join("Programs");
        candidates.push(lad.join(def.name).join(def.product_exe));
        candidates.push(lad.join(def.folder_name).join(def.product_exe));
    }
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        let pf = PathBuf::from(program_files);
        candidates.push(pf.join(def.name).join(def.product_exe));
        candidates.push(pf.join("Duke Systems").join(def.product_exe));
    }

    candidates.into_iter().find(|p| p.is_file())
}

#[derive(Clone, Debug)]
struct ProcEntry {
    pid: u32,
    parent_pid: u32,
    exe_lower: String,
}

#[cfg(target_os = "windows")]
fn snapshot_running_processes() -> Vec<ProcEntry> {
    let mut list = Vec::with_capacity(256);
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return list;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let exe_name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
                if entry.th32ProcessID > 4 {
                    list.push(ProcEntry {
                        pid: entry.th32ProcessID,
                        parent_pid: entry.th32ParentProcessID,
                        exe_lower: exe_name,
                    });
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    list
}

#[cfg(not(target_os = "windows"))]
fn snapshot_running_processes() -> Vec<ProcEntry> {
    Vec::new()
}

#[cfg(target_os = "windows")]
fn enumerate_visible_windows() -> HashMap<u32, isize> {
    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        if GetWindowTextLengthW(hwnd) <= 0 {
            return BOOL(1);
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid > 4 {
            let map = &mut *(lparam.0 as *mut HashMap<u32, isize>);
            map.entry(pid).or_insert(hwnd.0 as isize);
        }
        BOOL(1)
    }

    let mut map: HashMap<u32, isize> = HashMap::new();
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut map as *mut _ as isize));
    }
    map
}

#[cfg(not(target_os = "windows"))]
fn enumerate_visible_windows() -> HashMap<u32, isize> {
    HashMap::new()
}

#[cfg(target_os = "windows")]
fn focus_hwnd(raw_hwnd: isize) {
    unsafe {
        let hwnd = HWND(raw_hwnd as *mut core::ffi::c_void);
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = SetForegroundWindow(hwnd);
    }
}

#[cfg(not(target_os = "windows"))]
fn focus_hwnd(_raw_hwnd: isize) {}

fn matches_suite_def(exe_lower: &str, def: &SuiteAppDef) -> bool {
    exe_lower == def.product_exe.to_lowercase() || exe_lower == def.cargo_exe.to_lowercase()
}

fn build_suite_status_snapshot(app: &AppHandle, state: &SharedState) -> SuiteStatusSnapshot {
    let procs = snapshot_running_processes();
    let visible_windows = enumerate_visible_windows();

    let mut apps = Vec::with_capacity(SUITE_APPS.len());
    let mut apps_ram_mb = 0.0f64;
    let mut running_count = 0usize;
    let mut installed_count = 0usize;

    for def in &SUITE_APPS {
        let exe_opt = discover_app_exe(def);
        let installed = exe_opt.is_some();
        if installed {
            installed_count += 1;
        }
        let exe_path = exe_opt
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let matching_pids: Vec<u32> = procs
            .iter()
            .filter(|p| matches_suite_def(&p.exe_lower, def))
            .map(|p| p.pid)
            .collect();

        let running = !matching_pids.is_empty();
        if running {
            running_count += 1;
        }

        let primary_pid = matching_pids
            .iter()
            .copied()
            .find(|pid| visible_windows.contains_key(pid))
            .or_else(|| matching_pids.first().copied());

        let window_open = matching_pids
            .iter()
            .any(|pid| visible_windows.contains_key(pid));

        let ram_mb = if running {
            let sum: f64 = matching_pids
                .iter()
                .map(|pid| get_external_working_set_mb(*pid))
                .sum();
            round_tenths(sum)
        } else {
            0.0
        };
        apps_ram_mb += ram_mb;

        let status_label = if !running {
            "OFFLINE".to_string()
        } else if window_open {
            "ACTIVE WINDOW".to_string()
        } else {
            "RUNNING IN TRAY".to_string()
        };

        apps.push(SuiteAppStatus {
            id: def.id.to_string(),
            name: def.name.to_string(),
            subtitle: def.subtitle.to_string(),
            logo_asset: def.logo_asset.to_string(),
            installed,
            exe_path,
            running,
            pid: primary_pid,
            ram_mb,
            window_open,
            status_label,
        });
    }

    apps_ram_mb = round_tenths(apps_ram_mb);
    let own_ram_mb = round_tenths(get_current_working_set_mb());
    let total_suite_ram_mb = round_tenths(apps_ram_mb + own_ram_mb);

    let (pipe_bus_online, pipe_bus_is_hub) = DUKE_BUS
        .get()
        .map(|bus| (bus.is_online(), bus.is_hub()))
        .unwrap_or((false, false));

    let shm_bus_ref = DUKE_SHM_BUS.get().and_then(|o| o.as_ref());
    let shm_bus_active = shm_bus_ref.is_some();
    let shm_total_frames = shm_bus_ref.map(|b| b.write_cursor()).unwrap_or(0);
    let shm_active_apps: Vec<String> = shm_bus_ref
        .map(|b| {
            b.active_apps()
                .into_iter()
                .map(|a| a.display_name().to_string())
                .collect()
        })
        .unwrap_or_default();

    let has_shm_tick = state.shm_last_tick_ms.load(Ordering::Acquire) > 0;
    let shm_gpu_temp_c = if has_shm_tick {
        let raw = state.shm_gpu_temp_c.load(Ordering::Acquire);
        if raw == u64::from(u32::MAX) {
            None
        } else {
            Some(raw as u32)
        }
    } else {
        None
    };
    let shm_gpu_load_pct = if has_shm_tick {
        let raw = state.shm_gpu_load_pct.load(Ordering::Acquire);
        if raw == u64::from(u32::MAX) {
            None
        } else {
            Some(raw as u32)
        }
    } else {
        None
    };
    let shm_sys_ram_used_mb = if has_shm_tick {
        Some(state.shm_sys_ram_used_mb.load(Ordering::Acquire))
    } else {
        None
    };

    let settings = state
        .settings
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default();

    let activity_feed = state
        .activity_feed
        .lock()
        .map(|f| f.iter().cloned().collect())
        .unwrap_or_default();

    SuiteStatusSnapshot {
        apps,
        total_suite_ram_mb,
        apps_ram_mb,
        own_ram_mb,
        running_count,
        installed_count,
        pipe_bus_online,
        pipe_bus_is_hub,
        pipe_name: DUKE_PIPE_NAME.to_string(),
        shm_bus_active,
        shm_name: DUKE_SHM_NAME.to_string(),
        shm_total_frames,
        shm_ring_capacity: SHM_RING_CAPACITY,
        shm_active_apps,
        shm_gpu_temp_c,
        shm_gpu_load_pct,
        shm_sys_ram_used_mb,
        portable_mode: resolve_portable_data_dir().is_some(),
        settings_path: get_settings_path(app).to_string_lossy().to_string(),
        settings,
        activity_feed,
    }
}

fn spawn_suite_app_sync(def: &SuiteAppDef) -> Result<(u32, PathBuf), String> {
    let exe_path = discover_app_exe(def)
        .ok_or_else(|| format!("{} executable not found in workspace or install folder.", def.name))?;

    let mut cmd = std::process::Command::new(&exe_path);
    if let Some(parent) = exe_path.parent() {
        cmd.current_dir(parent);
    }

    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);

    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed launching {}: {}", def.name, e))?;
    Ok((child.id(), exe_path))
}

fn launch_or_focus_suite_app_sync(
    app: &AppHandle,
    state: &SharedState,
    app_id: &str,
) -> Result<String, String> {
    let def = find_suite_app_def(app_id)
        .ok_or_else(|| format!("Unknown Duke Suite app id '{}'.", app_id))?;

    let procs = snapshot_running_processes();
    let visible_windows = enumerate_visible_windows();
    let running_pids: Vec<u32> = procs
        .iter()
        .filter(|p| matches_suite_def(&p.exe_lower, def))
        .map(|p| p.pid)
        .collect();

    if !running_pids.is_empty() {
        if let Some(hwnd) = running_pids
            .iter()
            .find_map(|pid| visible_windows.get(pid).copied())
        {
            focus_hwnd(hwnd);
            return Ok(format!("Focused active {} window.", def.name));
        }
        return Err(format!(
            "{} is already running in the system tray (use its tray icon to reopen, or Stop first).",
            def.name
        ));
    }

    let (pid, _) = spawn_suite_app_sync(def)?;
    push_activity(
        state,
        Some(app),
        "SuiteAction",
        "duke-systems",
        format!("Launched {} (PID {})", def.name, pid),
    );
    let _ = app.emit("suite-status-changed", ());
    Ok(format!("Launched {} (PID {}).", def.name, pid))
}

async fn launch_suite_sequence(
    app: &AppHandle,
    state: &Arc<SharedState>,
    order: &[&str],
) -> LaunchSuiteSummary {
    let mut launched = Vec::new();
    let mut skipped = Vec::new();
    let mut missing = Vec::new();

    for (idx, app_id) in order.iter().enumerate() {
        let Some(def) = find_suite_app_def(app_id) else {
            continue;
        };

        let already_running = {
            let procs = snapshot_running_processes();
            procs.iter().any(|p| matches_suite_def(&p.exe_lower, def))
        };

        if already_running {
            skipped.push(def.name.to_string());
            continue;
        }

        match spawn_suite_app_sync(def) {
            Ok((pid, _)) => {
                launched.push(def.name.to_string());
                push_activity(
                    state,
                    Some(app),
                    "SuiteAction",
                    "duke-systems",
                    format!("Suite launch started {} (PID {})", def.name, pid),
                );
                let _ = app.emit("suite-status-changed", ());
                if idx + 1 < order.len() {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            }
            Err(_) => {
                missing.push(def.name.to_string());
            }
        }
    }

    let message = if !launched.is_empty() {
        format!(
            "Launched {} ({}).",
            launched.join(", "),
            if skipped.is_empty() {
                "sequence complete".to_string()
            } else {
                format!("{} already running", skipped.len())
            }
        )
    } else if !skipped.is_empty() {
        format!("All {} installed suite apps are already running.", skipped.len())
    } else {
        "No suite executables found to launch.".to_string()
    };

    LaunchSuiteSummary {
        launched,
        skipped,
        missing,
        message,
    }
}

#[cfg(target_os = "windows")]
fn terminate_pid_sync(pid: u32) -> Result<(), String> {
    if pid <= 4 || pid == std::process::id() {
        return Err("Refused to terminate protected system/self PID.".to_string());
    }
    unsafe {
        let handle: HANDLE = OpenProcess(PROCESS_TERMINATE, false, pid)
            .map_err(|e| format!("Access denied terminating PID {}: {}", pid, e))?;
        let res = TerminateProcess(handle, 0);
        let _ = CloseHandle(handle);
        res.map_err(|e| format!("TerminateProcess failed for PID {}: {}", pid, e))?;
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn terminate_pid_sync(_pid: u32) -> Result<(), String> {
    Err("Process termination is only supported on Windows.".to_string())
}

fn purge_suite_ram_sync(app: &AppHandle, state: &SharedState) -> RamPurgeSummary {
    if let Some(Some(shm)) = DUKE_SHM_BUS.get() {
        shm.publish_event(
            ShmEventType::RamPurge,
            ShmSourceApp::DukeSystems,
            std::process::id(),
            0,
            0,
            "suite-purge",
        );
    }
    if let Some(bus) = DUKE_BUS.get() {
        bus.broadcast(DukeEvent::RamPurgeRequested {
            source: "duke-systems".to_string(),
        });
    }

    let procs = snapshot_running_processes();
    let self_pid = std::process::id();

    let mut duke_pids: HashSet<u32> = HashSet::new();
    for p in &procs {
        if SUITE_APPS
            .iter()
            .any(|def| matches_suite_def(&p.exe_lower, def))
        {
            duke_pids.insert(p.pid);
        }
    }

    // Also include child msedgewebview2.exe processes spawned by running Duke Suite apps
    let mut trim_targets: Vec<u32> = duke_pids.iter().copied().collect();
    for p in &procs {
        if p.exe_lower == "msedgewebview2.exe"
            && (duke_pids.contains(&p.parent_pid) || p.parent_pid == self_pid)
        {
            trim_targets.push(p.pid);
        }
    }

    let mut total_freed_mb = 0.0f64;
    let mut trimmed_count = 0usize;

    for pid in trim_targets {
        if pid == self_pid {
            continue;
        }
        if let Ok(freed) = trim_external_pid(pid) {
            total_freed_mb += freed.max(0.0);
            trimmed_count += 1;
        }
    }

    let self_before = get_current_working_set_mb();
    trim_current_working_set();
    let self_after = get_current_working_set_mb();
    total_freed_mb += (self_before - self_after).max(0.0);
    trimmed_count += 1;

    let freed_mb = round_tenths(total_freed_mb);
    let own_ram_mb = round_tenths(self_after);

    let msg = format!(
        "Broadcast RamPurgeRequested & trimmed {} processes ({:.1} MB freed).",
        trimmed_count, freed_mb
    );
    push_activity(
        state,
        Some(app),
        "RamPurgeRequested",
        "duke-systems",
        msg.clone(),
    );
    let _ = app.emit("suite-status-changed", ());

    RamPurgeSummary {
        freed_mb,
        processes_trimmed: trimmed_count,
        own_ram_mb,
        message: msg,
    }
}

#[tauri::command]
async fn get_suite_status(
    app: AppHandle,
    state: tauri::State<'_, Arc<SharedState>>,
) -> Result<SuiteStatusSnapshot, String> {
    let app_clone = app.clone();
    let state_clone = state.inner().clone();
    tokio::task::spawn_blocking(move || build_suite_status_snapshot(&app_clone, &state_clone))
        .await
        .map_err(|e| format!("Failed collecting suite status: {}", e))
}

#[tauri::command]
async fn launch_suite_app(
    app: AppHandle,
    state: tauri::State<'_, Arc<SharedState>>,
    app_id: String,
) -> Result<String, String> {
    let app_clone = app.clone();
    let state_clone = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        launch_or_focus_suite_app_sync(&app_clone, &state_clone, &app_id)
    })
    .await
    .map_err(|e| format!("Launch task failed: {}", e))?
}

#[tauri::command]
async fn launch_full_suite(
    app: AppHandle,
    state: tauri::State<'_, Arc<SharedState>>,
) -> Result<LaunchSuiteSummary, String> {
    Ok(launch_suite_sequence(&app, state.inner(), &SUITE_LAUNCH_ORDER).await)
}

#[tauri::command]
async fn stop_suite_app(
    app: AppHandle,
    state: tauri::State<'_, Arc<SharedState>>,
    app_id: String,
) -> Result<String, String> {
    let def = find_suite_app_def(&app_id)
        .ok_or_else(|| format!("Unknown Duke Suite app id '{}'.", app_id))?;

    let app_clone = app.clone();
    let state_clone = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        let procs = snapshot_running_processes();
        let pids: Vec<u32> = procs
            .iter()
            .filter(|p| matches_suite_def(&p.exe_lower, def))
            .map(|p| p.pid)
            .collect();

        if pids.is_empty() {
            return Err(format!("{} is not currently running.", def.name));
        }

        let mut stopped = 0usize;
        for pid in &pids {
            terminate_pid_sync(*pid)?;
            stopped += 1;
        }

        let msg = format!(
            "Stopped {} ({} process{} terminated).",
            def.name,
            stopped,
            if stopped == 1 { "" } else { "es" }
        );
        push_activity(
            &state_clone,
            Some(&app_clone),
            "SuiteAction",
            "duke-systems",
            msg.clone(),
        );
        let _ = app_clone.emit("suite-status-changed", ());
        Ok(msg)
    })
    .await
    .map_err(|e| format!("Stop task failed: {}", e))?
}

#[tauri::command]
async fn purge_suite_ram(
    app: AppHandle,
    state: tauri::State<'_, Arc<SharedState>>,
) -> Result<RamPurgeSummary, String> {
    let app_clone = app.clone();
    let state_clone = state.inner().clone();
    tokio::task::spawn_blocking(move || purge_suite_ram_sync(&app_clone, &state_clone))
        .await
        .map_err(|e| format!("RAM purge task failed: {}", e))
}

#[tauri::command]
async fn open_app_folder(app_id: String) -> Result<(), String> {
    let def = find_suite_app_def(&app_id)
        .ok_or_else(|| format!("Unknown Duke Suite app id '{}'.", app_id))?;

    let exe_path = discover_app_exe(def)
        .ok_or_else(|| format!("Could not locate {} on disk.", def.name))?;

    #[cfg(target_os = "windows")]
    {
        let select_arg = format!("/select,{}", exe_path.display());
        std::process::Command::new("explorer.exe")
            .arg(select_arg)
            .spawn()
            .map_err(|e| format!("Failed opening Explorer: {}", e))?;
    }
    #[cfg(not(target_os = "windows"))]
    let _ = exe_path;

    Ok(())
}

#[tauri::command]
async fn update_suite_settings(
    app: AppHandle,
    state: tauri::State<'_, Arc<SharedState>>,
    settings: SuiteSettings,
) -> Result<SuiteSettings, String> {
    let previous_autostart = state
        .settings
        .lock()
        .map(|s| s.autostart_windows)
        .unwrap_or(false);

    if previous_autostart != settings.autostart_windows {
        set_windows_autostart(settings.autostart_windows)?;
    }

    state
        .daemon
        .config
        .close_to_tray
        .store(settings.close_to_tray, Ordering::Relaxed);
    state
        .daemon
        .config
        .minimize_to_tray
        .store(settings.minimize_to_tray, Ordering::Relaxed);

    if let Ok(mut guard) = state.settings.lock() {
        *guard = settings.clone();
    }
    save_settings(&app, &settings)?;
    Ok(settings)
}

#[tauri::command]
async fn clear_activity_feed(
    app: AppHandle,
    state: tauri::State<'_, Arc<SharedState>>,
) -> Result<(), String> {
    if let Ok(mut feed) = state.activity_feed.lock() {
        feed.clear();
    }
    let _ = app.emit("suite-status-changed", ());
    Ok(())
}

#[tauri::command]
async fn trim_working_set() -> Result<serde_json::Value, String> {
    trim_current_working_set();
    let ram_mb = round_tenths(get_current_working_set_mb());
    Ok(serde_json::json!({
        "workingSetMb": ram_mb
    }))
}

#[tauri::command]
fn set_ui_visible(state: tauri::State<'_, Arc<SharedState>>, visible: bool) {
    state.ui_visible.store(visible, Ordering::Release);
    state.daemon.ui_visible.store(visible, Ordering::Release);
    if visible {
        restore_normal_priority_mode();
    } else {
        enable_eco_qos_mode();
        trim_current_working_set();
    }
}

fn format_bus_event(event: &DukeEvent) -> (&'static str, String, String) {
    match event {
        DukeEvent::GameStarted {
            title,
            exe_name,
            pid,
        } => {
            let label = if title.trim().is_empty() {
                exe_name.as_str()
            } else {
                title.as_str()
            };
            (
                "GameStarted",
                "duke-launcher".to_string(),
                format!("Started \"{}\" ({}, PID {})", label, exe_name, pid),
            )
        }
        DukeEvent::GameStopped {
            title,
            exe_name,
            pid,
        } => {
            let label = if title.trim().is_empty() {
                exe_name.as_str()
            } else {
                title.as_str()
            };
            (
                "GameStopped",
                "duke-launcher".to_string(),
                format!("Stopped \"{}\" ({}, PID {})", label, exe_name, pid),
            )
        }
        DukeEvent::ProfileSwitched { app, profile_name } => (
            "ProfileSwitched",
            "duke-input".to_string(),
            format!("{} switched profile to \"{}\"", app, profile_name),
        ),
        DukeEvent::ClipSaved {
            game_title,
            file_path,
            duration_secs,
        } => (
            "ClipSaved",
            "duke-capture".to_string(),
            format!(
                "Saved {:.1}s clip for {} ({})",
                duration_secs, game_title, file_path
            ),
        ),
        DukeEvent::RamPurgeRequested { source } => (
            "RamPurgeRequested",
            "pipe-ipc".to_string(),
            format!("RAM purge requested by {}", source),
        ),
    }
}

fn install_suite_tray_menu(app: &AppHandle, state: Arc<SharedState>) -> tauri::Result<()> {
    let Some(tray) = app.tray_by_id("duke-systems-tray") else {
        return Ok(());
    };

    let open_hub = MenuItemBuilder::with_id("duke_tray_open", "Open Duke Systems").build(app)?;
    let launch_all =
        MenuItemBuilder::with_id("duke_tray_launch_suite", "Launch Full Suite").build(app)?;
    let purge_ram =
        MenuItemBuilder::with_id("duke_tray_purge_suite", "Purge Suite RAM").build(app)?;
    let open_launcher =
        MenuItemBuilder::with_id("duke_tray_open_launcher", "Open Duke Launcher").build(app)?;
    let open_monitor =
        MenuItemBuilder::with_id("duke_tray_open_monitor", "Open Duke Monitor").build(app)?;
    let open_capture =
        MenuItemBuilder::with_id("duke_tray_open_capture", "Open Duke Capture").build(app)?;
    let open_input =
        MenuItemBuilder::with_id("duke_tray_open_input", "Open Duke Input").build(app)?;
    let exit_item = MenuItemBuilder::with_id("duke_tray_exit", "Exit").build(app)?;

    let menu = MenuBuilder::new(app)
        .item(&open_hub)
        .item(&launch_all)
        .item(&purge_ram)
        .separator()
        .item(&open_launcher)
        .item(&open_monitor)
        .item(&open_capture)
        .item(&open_input)
        .separator()
        .item(&exit_item)
        .build()?;

    tray.set_menu(Some(menu))?;

    app.on_menu_event(move |app_handle, event| {
        let id = event.id().as_ref();
        match id {
            "duke_tray_launch_suite" => {
                let h = app_handle.clone();
                let st = state.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = launch_suite_sequence(&h, &st, &SUITE_LAUNCH_ORDER).await;
                });
            }
            "duke_tray_purge_suite" => {
                let h = app_handle.clone();
                let st = state.clone();
                std::thread::spawn(move || {
                    let _ = purge_suite_ram_sync(&h, &st);
                });
            }
            "duke_tray_open_launcher" => {
                let h = app_handle.clone();
                let st = state.clone();
                std::thread::spawn(move || {
                    let _ = launch_or_focus_suite_app_sync(&h, &st, "launcher");
                });
            }
            "duke_tray_open_monitor" => {
                let h = app_handle.clone();
                let st = state.clone();
                std::thread::spawn(move || {
                    let _ = launch_or_focus_suite_app_sync(&h, &st, "monitor");
                });
            }
            "duke_tray_open_capture" => {
                let h = app_handle.clone();
                let st = state.clone();
                std::thread::spawn(move || {
                    let _ = launch_or_focus_suite_app_sync(&h, &st, "capture");
                });
            }
            "duke_tray_open_input" => {
                let h = app_handle.clone();
                let st = state.clone();
                std::thread::spawn(move || {
                    let _ = launch_or_focus_suite_app_sync(&h, &st, "input");
                });
            }
            _ => {}
        }
    });

    Ok(())
}

fn main() {
    let launch_minimized_arg = std::env::args().any(|a| a == "--minimized");

    let daemon = TrayDaemonState::new(TrayDaemonConfig {
        tray_id: "duke-systems-tray".to_string(),
        window_label: "main".to_string(),
        tooltip: "Duke Systems - Suite Command Hub".to_string(),
        open_label: "Open Duke Systems".to_string(),
        trim_label: "Purge Suite RAM".to_string(),
        exit_label: "Exit".to_string(),
        close_to_tray: Arc::new(AtomicBool::new(true)),
        minimize_to_tray: Arc::new(AtomicBool::new(true)),
    });
    let daemon_setup = daemon.clone();
    let daemon_run = daemon;

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .setup(move |app| {
            let settings = load_settings(app.handle());
            daemon_setup
                .config
                .close_to_tray
                .store(settings.close_to_tray, Ordering::Relaxed);
            daemon_setup
                .config
                .minimize_to_tray
                .store(settings.minimize_to_tray, Ordering::Relaxed);

            let start_minimized = launch_minimized_arg || settings.start_minimized;
            let state = Arc::new(SharedState::new(settings.clone(), daemon_setup.clone()));
            app.manage(state.clone());

            // Wire standard duke-core tray daemon + full suite tray menu
            let on_show_state = state.clone();
            let on_exit_bus = Arc::new(|| {
                SHM_STOP_SIGNAL.store(true, Ordering::Release);
                if let Some(Some(shm)) = DUKE_SHM_BUS.get() {
                    shm.unregister_app();
                }
                if let Some(bus) = DUKE_BUS.get() {
                    bus.stop();
                }
            });
            install_standard_tray(
                app.handle(),
                &daemon_setup,
                Some(Arc::new(move || {
                    on_show_state.ui_visible.store(true, Ordering::Release);
                    restore_normal_priority_mode();
                })),
                Some(on_exit_bus),
            )?;
            install_suite_tray_menu(app.handle(), state.clone())?;

            // Connect to lock-free Shared Memory Ring Buffer (`Local\duke-suite-shm-v1`)
            let shm_ready = DUKE_SHM_BUS
                .get_or_init(|| {
                    let bus = DukeShmBus::open_or_create().ok();
                    if let Some(ref b) = bus {
                        b.register_app(ShmSourceApp::DukeSystems);
                    }
                    bus
                })
                .is_some();

            if shm_ready {
                let shm_app = app.handle().clone();
                let shm_state = state.clone();
                let _ = std::thread::Builder::new()
                    .name("duke-systems-shm-rx".to_string())
                    .spawn(move || {
                        let Ok(mut shm_rx) = DukeShmBus::open_or_create() else {
                            return;
                        };
                        while !SHM_STOP_SIGNAL.load(Ordering::Acquire) {
                            let mut notify_ui = false;
                            while let Some(frame) = shm_rx.try_recv() {
                                match frame.kind() {
                                    ShmEventType::TelemetryTick => {
                                        if frame.app() == ShmSourceApp::DukeMonitor {
                                            let (temp_c, load_pct) =
                                                unpack_gpu_telemetry(frame.payload_a);
                                            shm_state.shm_gpu_temp_c.store(
                                                u64::from(temp_c.unwrap_or(u32::MAX)),
                                                Ordering::Release,
                                            );
                                            shm_state.shm_gpu_load_pct.store(
                                                u64::from(load_pct.unwrap_or(u32::MAX)),
                                                Ordering::Release,
                                            );
                                            shm_state
                                                .shm_sys_ram_used_mb
                                                .store(frame.payload_b, Ordering::Release);
                                            shm_state
                                                .shm_last_tick_ms
                                                .store(current_unix_ms(), Ordering::Release);
                                        }
                                    }
                                    ShmEventType::GameLaunched
                                    | ShmEventType::GameExited
                                    | ShmEventType::RamPurge => {
                                        notify_ui = true;
                                    }
                                    _ => {}
                                }
                            }
                            if notify_ui {
                                let _ = shm_app.emit("suite-status-changed", ());
                            }
                            std::thread::sleep(Duration::from_millis(25));
                        }
                    });

                push_activity(
                    &state,
                    Some(app.handle()),
                    "SuiteAction",
                    "duke-shm",
                    format!(
                        "Shared Memory Ring Buffer ACTIVE on {} ({} slots)",
                        DUKE_SHM_NAME, SHM_RING_CAPACITY
                    ),
                );
            }

            // Start persistent \\.\pipe\duke-suite Named Pipe Bus hub
            let bus_app = app.handle().clone();
            let bus_state = state.clone();
            let _ = DUKE_BUS.get_or_init(|| {
                DukeBus::start("duke-systems", move |event| {
                    let (ev_type, ev_source, ev_summary) = format_bus_event(&event);
                    push_activity(&bus_state, Some(&bus_app), ev_type, ev_source, ev_summary);
                    if matches!(event, DukeEvent::RamPurgeRequested { .. }) {
                        trim_current_working_set();
                    }
                })
            });

            push_activity(
                &state,
                Some(app.handle()),
                "SuiteAction",
                "duke-systems",
                format!("Duke Systems bus initialized on {}", DUKE_PIPE_NAME),
            );

            // Auto-launch any suite apps enabled in IPC & Automation settings
            let auto_ids: Vec<&'static str> = SUITE_LAUNCH_ORDER
                .iter()
                .copied()
                .filter(|id| settings.auto_launch.is_enabled(id))
                .collect();
            if !auto_ids.is_empty() {
                let h = app.handle().clone();
                let st = state.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(350)).await;
                    let _ = launch_suite_sequence(&h, &st, &auto_ids).await;
                });
            }

            // Periodic 60s idle working-set maintenance
            let trim_app = app.handle().clone();
            let trim_state = state.clone();
            tauri::async_runtime::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_secs(60));
                loop {
                    ticker.tick().await;
                    let auto_trim = trim_state
                        .settings
                        .lock()
                        .map(|s| s.auto_trim_enabled)
                        .unwrap_or(true);
                    if auto_trim {
                        trim_current_working_set();
                    }
                    let _ = trim_app.emit(
                        "memory-telemetry",
                        serde_json::json!({
                            "ownRamMb": round_tenths(get_current_working_set_mb())
                        }),
                    );
                }
            });

            if start_minimized {
                state.ui_visible.store(false, Ordering::Release);
                daemon_setup.ui_visible.store(false, Ordering::Release);
                enable_eco_qos_mode();
                destroy_window_for_tray(app.handle(), "main");
            } else {
                trim_current_working_set();
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_suite_status,
            launch_suite_app,
            launch_full_suite,
            stop_suite_app,
            purge_suite_ram,
            open_app_folder,
            update_suite_settings,
            clear_activity_feed,
            trim_working_set,
            set_ui_visible,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Duke Systems Tauri application");

    let run_state = app.try_state::<Arc<SharedState>>().map(|s| s.inner().clone());

    app.run(move |app_handle, event| {
        let on_hide_cb = || {
            if let Some(st) = run_state.as_ref() {
                st.ui_visible.store(false, Ordering::Release);
            }
            enable_eco_qos_mode();
        };
        let on_exit_cb = || {
            SHM_STOP_SIGNAL.store(true, Ordering::Release);
            if let Some(Some(shm)) = DUKE_SHM_BUS.get() {
                shm.unregister_app();
            }
            if let Some(bus) = DUKE_BUS.get() {
                bus.stop();
            }
        };
        handle_run_event(
            app_handle,
            &event,
            &daemon_run,
            Some(&on_hide_cb),
            Some(&on_exit_cb),
        );
    });
}
