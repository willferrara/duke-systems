#![allow(unused_imports)]
use duke_core::pipe_ipc::{DukeBus, DukeEvent};
use duke_core::shm_bus::{DukeShmBus, ShmEventType, ShmSourceApp};
use reqwest::header::{
    ACCEPT, ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, LOCATION,
    RANGE, REFERER, SET_COOKIE, USER_AGENT,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::windows::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::webview::{DownloadEvent, NewWindowResponse};
use tauri::{
    AppHandle, Emitter, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::{oneshot, Semaphore};
use walkdir::WalkDir;
use crate::{
    achievements, archive_native, artwork, bench, bios_vault, catalog_mmap, couch_mode,
    deep_shrink, delta_patch, desktop_chord, direct_storage, discord_ipc, dkpack, duke_acoustics,
    duke_ai_rt,
    duke_capsule, duke_comms, duke_configurator, duke_director, duke_doctor, duke_ghost,
    duke_jail, duke_link, duke_pilot, duke_sandbox, duke_upscale, duke_verified, duke_vision,
    emulators, etw_xray, fix_finder, friends, fse_shell, importers, latency_lab, lib_store,
    ludusavi, mcp, metadata_ext, mod_dropper, mod_graph, net_engine, ntfs_compress, pe_inspect,
    platforms, playtime_sync, privacy_vault, projfs_stream, quick_resume, redist_cleaner,
    retro_achievements, retro_catalog, rom_inspect, rom_patcher, rom_shrink, save_watcher,
    self_heal, shader_swarm, sidebar_icons, sites, snapshots, sources, steam_cm, store_direct,
    stream_extract, stress_harness, swarm, time_machine, uninstaller, updater, vhdx_stage,
    wasm_plugins,
};
use crate::cmd_emulators::*;
use crate::cmd_library::*;
use crate::cmd_security::*;
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ActiveGameSession {
    pub game_id: String,
    pub title: String,
    pub exe_path: String,
    pub pid: u32,
    pub is_suspended: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SuspendStateResult {
    pub game_id: String,
    pub title: String,
    pub pid: u32,
    pub is_suspended: bool,
    pub freed_ram_mb: f64,
}

pub(crate) static DUKE_BUS: OnceLock<DukeBus> = OnceLock::new();
pub(crate) static DUKE_SHM_BUS: OnceLock<Mutex<Option<DukeShmBus>>> = OnceLock::new();

pub(crate) fn publish_shm_event_tagged(
    event_type: ShmEventType,
    pid: u32,
    payload_a: u64,
    payload_b: u64,
    tag: &str,
) {
    if let Some(cell) = DUKE_SHM_BUS.get() {
        if let Ok(guard) = cell.lock() {
            if let Some(ref bus) = *guard {
                bus.publish_event(
                    event_type,
                    ShmSourceApp::DukeLauncher,
                    pid,
                    payload_a,
                    payload_b,
                    tag,
                );
            }
        }
    }
}

pub(crate) fn publish_shm_event(event_type: ShmEventType, pid: u32, payload_a: u64, payload_b: u64) {
    publish_shm_event_tagged(event_type, pid, payload_a, payload_b, "");
}

pub(crate) fn publish_shm_frame_direct(frame: duke_core::shm_bus::ShmEventFrame) {
    if let Some(cell) = DUKE_SHM_BUS.get() {
        if let Ok(guard) = cell.lock() {
            if let Some(ref bus) = *guard {
                bus.publish_frame(frame);
            }
        }
    }
}

pub(crate) fn publish_pipe_event(event: DukeEvent) {
    if let Some(bus) = DUKE_BUS.get() {
        bus.publish(event);
    }
}

pub(crate) fn active_shm_apps() -> Vec<ShmSourceApp> {
    if let Some(cell) = DUKE_SHM_BUS.get() {
        if let Ok(guard) = cell.lock() {
            if let Some(ref bus) = *guard {
                return bus.active_apps();
            }
        }
    }
    Vec::new()
}

#[tauri::command]
pub(crate) async fn get_suite_autonomy_status() -> Result<Value, String> {
    run_blocking_timeout(5, "get_suite_autonomy_status", || {
        let active = active_shm_apps();
        let shm_ready = DUKE_SHM_BUS
            .get()
            .and_then(|cell| cell.lock().ok().map(|g| g.is_some()))
            .unwrap_or(false);
        let pipe_online = DUKE_BUS.get().map(|b| b.is_online()).unwrap_or(false);

        let specs = [
            (
                "monitor",
                "Monitor (Optional)",
                "Standalone telemetry built-in",
                ShmSourceApp::DukeMonitor,
            ),
            (
                "capture",
                "Capture (Optional)",
                "Standalone highlight log active",
                ShmSourceApp::DukeCapture,
            ),
            (
                "input",
                "Input (Optional)",
                "Standalone XInput/QUIC built-in",
                ShmSourceApp::DukeInput,
            ),
        ];

        let mut daemons = Vec::with_capacity(specs.len());
        let mut connected_count = 0usize;
        for (id, name, role, app_tag) in specs {
            let shm_linked = active.contains(&app_tag);
            if shm_linked {
                connected_count += 1;
            }
            let status_text = if shm_linked && pipe_online {
                "SHM + Pipe Sync"
            } else if shm_linked {
                "SHM Linked"
            } else {
                "Standalone Ready"
            };
            daemons.push(serde_json::json!({
                "id": id,
                "name": name,
                "role": role,
                "connected": shm_linked,
                "shmRegistered": shm_linked,
                "pipeOnline": pipe_online && shm_linked,
                "statusText": status_text,
            }));
        }

        Ok(serde_json::json!({
            "standalone": true,
            "shmReady": shm_ready,
            "pipeOnline": pipe_online,
            "connectedCount": connected_count,
            "totalCount": specs.len(),
            "daemons": daemons,
        }))
    })
    .await
}
pub(crate) static ACTIVE_SESSIONS: OnceLock<Mutex<HashMap<String, ActiveGameSession>>> = OnceLock::new();
pub(crate) static ACTIVE_JOBS: OnceLock<Mutex<HashMap<String, Arc<duke_core::win32_ffi::ProcessTreeJob>>>> =
    OnceLock::new();
pub(crate) static TRAY_DAEMON: OnceLock<duke_core::tray_daemon::TrayDaemonState> = OnceLock::new();
pub(crate) static HOTKEY_WATCHER_ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) static APP_SHUTDOWN: AtomicBool = AtomicBool::new(false);

pub(crate) fn get_active_jobs_map(
) -> &'static Mutex<HashMap<String, Arc<duke_core::win32_ffi::ProcessTreeJob>>> {
    ACTIVE_JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}
pub(crate) static ACTIVE_SUSPEND_HOTKEY: OnceLock<Mutex<String>> = OnceLock::new();
pub(crate) static SETTINGS_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
pub(crate) static RAM_TRIM_MODE: AtomicU64 = AtomicU64::new(1);
pub(crate) static AUTO_FREEZE_MINIMIZED: AtomicBool = AtomicBool::new(false);
pub(crate) static COUCH_MODE_ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Offloads blocking disk, Win32 FFI, or archive work off the Tauri async executor with a hard timeout.
pub(crate) async fn run_blocking_timeout<F, T>(
    timeout_secs: u64,
    task_label: &str,
    work: F,
) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    let secs = timeout_secs.max(1);
    let join = tokio::task::spawn_blocking(work);
    match tokio::time::timeout(Duration::from_secs(secs), join).await {
        Ok(Ok(res)) => res,
        Ok(Err(join_err)) => Err(format!("{task_label} worker panicked: {join_err}")),
        Err(_) => Err(format!("{task_label} timed out after {secs}s.")),
    }
}

pub(crate) fn get_active_sessions() -> &'static Mutex<HashMap<String, ActiveGameSession>> {
    ACTIVE_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn get_active_hotkey_cell() -> &'static Mutex<String> {
    ACTIVE_SUSPEND_HOTKEY.get_or_init(|| Mutex::new(load_settings_standalone().suspend_hotkey))
}

pub(crate) fn find_active_session_pid(game_id_or_title: &str) -> Option<u32> {
    let guard = get_active_sessions()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    guard
        .values()
        .find(|s| {
            s.game_id == game_id_or_title
                || s.title.eq_ignore_ascii_case(game_id_or_title)
                || s.exe_path.eq_ignore_ascii_case(game_id_or_title)
        })
        .map(|s| s.pid)
}

pub(crate) fn first_active_session_pid() -> Option<u32> {
    let guard = get_active_sessions()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    guard.values().next().map(|s| s.pid)
}

pub(crate) use duke_core::win32_ffi::apply_backdrop_to_hwnd;
#[cfg(test)]
pub(crate) use duke_core::win32_ffi::{
    apply_backdrop_with, resolve_backdrop_type_for_theme, DwmMargins,
    DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMSBT_NONE, DWMSBT_TRANSIENTWINDOW,
};

#[tauri::command]
pub(crate) async fn get_app_version() -> Result<String, String> {
    Ok(env!("CARGO_PKG_VERSION").to_string())
}

#[tauri::command]
pub(crate) async fn apply_window_backdrop(app: AppHandle, theme_id: String) -> Result<(), String> {
    run_blocking_timeout(5, "apply_window_backdrop", move || {
        let clean_theme = normalize_theme_id(&theme_id);
        if let Some(win) = app.get_webview_window("main") {
            #[cfg(target_os = "windows")]
            {
                let hwnd = win
                    .hwnd()
                    .map_err(|e| format!("Failed to read main window HWND: {e}"))?;
                let _ = apply_backdrop_to_hwnd(hwnd.0 as isize, &clean_theme)?;
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = (win, &clean_theme);
            }
        }
        if let Some(overlay) = app.get_webview_window("overlay") {
            let _ = overlay.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
            #[cfg(target_os = "windows")]
            if let Ok(hwnd) = overlay.hwnd() {
                let _ = apply_backdrop_to_hwnd(hwnd.0 as isize, "acrylic");
            }
        }
        Ok(())
    })
    .await
}

/// Native Windows 11 working-set RAM trimmer via `duke-core`.
pub fn trim_process_working_set() {
    duke_core::win32_ffi::trim_current_working_set();
}

/// Windows 11 EcoQoS & Hybrid Core Pinning via `duke-core`.
pub fn enable_ecoqos_game_boost_mode() {
    duke_core::win32_ffi::enable_eco_qos_mode();
    duke_core::win32_ffi::trim_current_working_set();
}

/// Restores `NORMAL_PRIORITY_CLASS` (`0x00000020`) and disables EcoQoS execution throttling via `duke-core`.
pub fn restore_normal_priority_mode() {
    duke_core::win32_ffi::restore_normal_priority_mode();
    duke_core::win32_ffi::trim_current_working_set();
}

pub fn get_process_working_set_mb() -> f64 {
    let mb = duke_core::win32_ffi::get_current_working_set_mb();
    if mb > 0.1 {
        mb
    } else {
        18.4
    }
}

#[allow(dead_code)]
pub type AppSettings = LauncherSettings;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LauncherSettings {
    #[serde(alias = "auto_clean_completed")]
    pub auto_clean_completed: bool,
    #[serde(alias = "steamgriddb_api_key")]
    pub steamgriddb_api_key: Option<String>,
    #[serde(alias = "suspend_hotkey")]
    pub suspend_hotkey: String,
    #[serde(alias = "couch_mode")]
    pub couch_mode: bool,
    #[serde(alias = "auto_detect_clipboard")]
    pub auto_detect_clipboard: bool,
    #[serde(alias = "nvme_prefetch_enabled")]
    pub nvme_prefetch_enabled: bool,
    #[serde(alias = "swarm_enabled")]
    pub swarm_enabled: bool,
    #[serde(alias = "swarm_node_name")]
    pub swarm_node_name: String,
    #[serde(alias = "swarm_peers")]
    pub swarm_peers: Vec<String>,
    #[serde(alias = "steam_api_key")]
    pub steam_api_key: Option<String>,
    #[serde(alias = "steam_id_override")]
    pub steam_id_override: Option<String>,
    #[serde(alias = "xbox_api_key")]
    pub xbox_api_key: Option<String>,
    #[serde(alias = "psn_npsso")]
    pub psn_npsso: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub steam_api_key_configured: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub xbox_api_key_configured: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub psn_npsso_configured: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub steamgriddb_api_key_configured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detected_steam_id: Option<String>,
    // --- Essentials UX Fields ---
    #[serde(alias = "default_save_dir")]
    pub default_save_dir: Option<String>,
    #[serde(alias = "download_speed_preset")]
    pub download_speed_preset: String,
    #[serde(alias = "compact_library_cards")]
    pub compact_library_cards: bool,
    #[serde(alias = "performance_boost_enabled")]
    pub performance_boost_enabled: bool,
    #[serde(alias = "auto_backup_saves_on_exit")]
    pub auto_backup_saves_on_exit: bool,
    // --- Advanced Engine Fields ---
    #[serde(alias = "cpu_priority_mode")]
    pub cpu_priority_mode: String,
    #[serde(alias = "ram_trim_mode")]
    pub ram_trim_mode: String,
    #[serde(alias = "auto_freeze_minimized")]
    pub auto_freeze_minimized: bool,
    #[serde(alias = "nvme_cache_budget_pct")]
    pub nvme_cache_budget_pct: u8,
    #[serde(alias = "doh_enabled")]
    pub doh_enabled: bool,
    #[serde(alias = "custom_proxy_url")]
    pub custom_proxy_url: Option<String>,
    #[serde(alias = "custom_speed_cap_mbps")]
    pub custom_speed_cap_mbps: f64,
    #[serde(alias = "auto_unblock_motw")]
    pub auto_unblock_motw: bool,
    #[serde(alias = "portable_sandbox_default")]
    pub portable_sandbox_default: bool,
    #[serde(alias = "max_restore_points")]
    pub max_restore_points: u32,
    #[serde(alias = "block_game_telemetry")]
    pub block_game_telemetry: bool,
    #[serde(alias = "global_launch_args")]
    pub global_launch_args: Option<String>,
    #[serde(alias = "swarm_auto_sync_saves")]
    pub swarm_auto_sync_saves: bool,
    #[serde(alias = "start_minimized_with_windows")]
    pub start_minimized_with_windows: bool,
    #[serde(default = "default_theme_id")]
    pub theme: String,
    #[serde(default = "default_card_zoom_px", alias = "card_zoom_px")]
    pub card_zoom_px: u32,
    #[serde(default = "default_true", alias = "projfsStreamingEnabled")]
    pub projfs_streaming_enabled: bool,
    #[serde(default = "default_true", alias = "shaderSwarmEnabled")]
    pub shader_swarm_enabled: bool,
    #[serde(default = "default_true", alias = "framePacingEnabled")]
    pub frame_pacing_enabled: bool,
    #[serde(default, alias = "customEmulatorPaths")]
    pub custom_emulator_paths: HashMap<String, String>,
    #[serde(default = "default_disk_pressure_gb", alias = "diskPressureThresholdGb")]
    pub disk_pressure_threshold_gb: f64,
    #[serde(default = "default_disk_pressure_days", alias = "diskPressureDaysThreshold")]
    pub disk_pressure_days_threshold: u32,
    #[serde(default = "default_true", alias = "discordRpcEnabled", alias = "discord_rpc_enabled")]
    pub discord_rpc_enabled: bool,
    #[serde(default = "default_true", alias = "realtimeSaveWatcherEnabled", alias = "realtime_save_watcher_enabled")]
    pub realtime_save_watcher_enabled: bool,
    #[serde(default = "default_true", alias = "desktopChordEnabled", alias = "desktop_chord_enabled")]
    pub desktop_chord_enabled: bool,
    #[serde(default, alias = "minimizeToTray", alias = "minimize_to_tray")]
    pub minimize_to_tray: bool,
    #[serde(default, alias = "closeToTray", alias = "close_to_tray")]
    pub close_to_tray: bool,
    #[serde(default = "default_true", alias = "autoCheckUpdates", alias = "auto_check_updates")]
    pub auto_check_updates: bool,
}

pub(crate) fn default_disk_pressure_gb() -> f64 {
    50.0
}

pub(crate) fn default_disk_pressure_days() -> u32 {
    60
}

pub(crate) fn default_true() -> bool {
    true
}

pub(crate) fn default_theme_id() -> String {
    "dark".to_string()
}

pub(crate) fn default_card_zoom_px() -> u32 {
    185
}

pub(crate) fn normalize_theme_id(raw: &str) -> String {
    let clean = raw.trim().to_ascii_lowercase();
    match clean.as_str() {
        "dark" | "light" | "minecraft" | "sunset" | "aerospace" | "cyberpunk" | "terminal"
        | "crimson" | "ocean" | "frost" => clean,
        _ => "dark".to_string(),
    }
}

pub(crate) fn normalize_loaded_settings(raw_json: &str) -> (LauncherSettings, bool) {
    let mut parsed = serde_json::from_str::<LauncherSettings>(raw_json).unwrap_or_default();
    let mut migrated = false;
    let trimmed_theme = parsed.theme.trim();
    if trimmed_theme.is_empty() {
        parsed.theme = "dark".to_string();
    } else {
        let normalized = normalize_theme_id(trimmed_theme);
        if normalized != parsed.theme {
            parsed.theme = normalized;
            migrated = true;
        }
    }
    if parsed.card_zoom_px == 0 {
        parsed.card_zoom_px = default_card_zoom_px();
    } else {
        let clamped = parsed.card_zoom_px.clamp(125, 280);
        if clamped != parsed.card_zoom_px {
            parsed.card_zoom_px = clamped;
            migrated = true;
        }
    }
    (parsed, migrated)
}

pub(crate) fn is_false(v: &bool) -> bool {
    !*v
}

impl LauncherSettings {
    pub fn to_ipc_safe(&self) -> Self {
        let mut copy = self.clone();
        copy.steam_api_key_configured = copy
            .steam_api_key
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        copy.xbox_api_key_configured = copy
            .xbox_api_key
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        copy.psn_npsso_configured = copy
            .psn_npsso
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        copy.steamgriddb_api_key_configured = copy
            .steamgriddb_api_key
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        copy.steam_api_key = None;
        copy.xbox_api_key = None;
        copy.psn_npsso = None;
        copy.steamgriddb_api_key = None;
        copy.detected_steam_id = friends::detect_local_steam_id64();
        copy
    }
}

impl Default for LauncherSettings {
    fn default() -> Self {
        Self {
            auto_clean_completed: true,
            steamgriddb_api_key: None,
            suspend_hotkey: "Ctrl+Alt+P".to_string(),
            couch_mode: false,
            auto_detect_clipboard: true,
            nvme_prefetch_enabled: true,
            swarm_enabled: false,
            swarm_node_name: String::new(),
            swarm_peers: Vec::new(),
            steam_api_key: None,
            steam_id_override: None,
            xbox_api_key: None,
            psn_npsso: None,
            steam_api_key_configured: false,
            xbox_api_key_configured: false,
            psn_npsso_configured: false,
            steamgriddb_api_key_configured: false,
            detected_steam_id: None,
            default_save_dir: None,
            download_speed_preset: "max".to_string(),
            compact_library_cards: false,
            performance_boost_enabled: true,
            auto_backup_saves_on_exit: false,
            cpu_priority_mode: "auto".to_string(),
            ram_trim_mode: "standard".to_string(),
            auto_freeze_minimized: false,
            nvme_cache_budget_pct: 25,
            doh_enabled: true,
            custom_proxy_url: None,
            custom_speed_cap_mbps: 0.0,
            auto_unblock_motw: true,
            portable_sandbox_default: false,
            max_restore_points: 5,
            block_game_telemetry: true,
            global_launch_args: None,
            swarm_auto_sync_saves: false,
            start_minimized_with_windows: false,
            theme: default_theme_id(),
            card_zoom_px: default_card_zoom_px(),
            projfs_streaming_enabled: true,
            shader_swarm_enabled: true,
            frame_pacing_enabled: true,
            custom_emulator_paths: HashMap::new(),
            disk_pressure_threshold_gb: default_disk_pressure_gb(),
            disk_pressure_days_threshold: default_disk_pressure_days(),
            discord_rpc_enabled: true,
            realtime_save_watcher_enabled: true,
            desktop_chord_enabled: true,
            minimize_to_tray: false,
            close_to_tray: false,
            auto_check_updates: true,
        }
    }
}

pub(crate) fn effective_speed_limit_kbps(settings: &LauncherSettings) -> u64 {
    if settings.custom_speed_cap_mbps > 0.0 {
        return (settings.custom_speed_cap_mbps * 1024.0) as u64;
    }
    match settings.download_speed_preset.trim().to_ascii_lowercase().as_str() {
        "balanced" => 15 * 1024,
        "background" => 3 * 1024,
        _ => 0,
    }
}

pub(crate) fn apply_runtime_settings(settings: &LauncherSettings) {
    let (doh_provider, vault_proxy) = privacy_vault::current_doh_and_proxy_overrides();
    duke_core::net_doh::set_doh_provider_mode(&doh_provider);
    let _ = duke_core::net_doh::set_network_privacy_config(
        settings.doh_enabled && doh_provider != "disabled",
        settings.custom_proxy_url.as_deref().or(vault_proxy.as_deref()),
    );
    GLOBAL_SPEED_LIMIT_KBPS.store(effective_speed_limit_kbps(settings), Ordering::Relaxed);
    let ram_mode = match settings.ram_trim_mode.trim().to_ascii_lowercase().as_str() {
        "off" => 0,
        "aggressive" => 2,
        _ => 1,
    };
    RAM_TRIM_MODE.store(ram_mode, Ordering::Relaxed);
    AUTO_FREEZE_MINIMIZED.store(settings.auto_freeze_minimized, Ordering::Relaxed);
    AUTO_UNBLOCK_MOTW.store(settings.auto_unblock_motw, Ordering::Relaxed);
    COUCH_MODE_ACTIVE.store(settings.couch_mode, Ordering::Release);
    discord_ipc::set_discord_presence_enabled(settings.discord_rpc_enabled);
    desktop_chord::set_desktop_chord_enabled(settings.desktop_chord_enabled);
    if !settings.realtime_save_watcher_enabled {
        save_watcher::stop_all_save_watchers();
    }
    if let Some(daemon) = TRAY_DAEMON.get() {
        daemon
            .config
            .minimize_to_tray
            .store(settings.minimize_to_tray, Ordering::Release);
        daemon
            .config
            .close_to_tray
            .store(settings.close_to_tray, Ordering::Release);
    }
    if let Ok(mut guard) = get_active_hotkey_cell().lock() {
        *guard = settings.suspend_hotkey.clone();
    }
}

pub(crate) fn get_settings_io_lock() -> &'static Mutex<()> {
    SETTINGS_IO_LOCK.get_or_init(|| Mutex::new(()))
}

pub(crate) fn load_settings_standalone() -> LauncherSettings {
    let _guard = get_settings_io_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let Ok(path) = get_app_data_file_standalone("settings.json") else {
        return LauncherSettings::default();
    };
    if !path.exists() {
        return LauncherSettings::default();
    }
    let Ok(raw) = fs::read_to_string(&path) else {
        return LauncherSettings::default();
    };
    if raw.trim().is_empty() {
        return LauncherSettings::default();
    }
    let (mut settings, migrated) = normalize_loaded_settings(&raw);
    let vault_migrated = privacy_vault::hydrate_settings_from_vault(&mut settings);
    if migrated || vault_migrated {
        let scrubbed = privacy_vault::extract_secrets_for_settings_save(&settings);
        if let Ok(serialized) = serde_json::to_string_pretty(&scrubbed) {
            let _ = atomic_write_string(&path, &serialized);
        }
    }
    settings
}

pub(crate) fn load_settings_from_disk(app: &AppHandle) -> LauncherSettings {
    let _guard = get_settings_io_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let Ok(path) = get_app_data_file(app, "settings.json") else {
        return LauncherSettings::default();
    };
    if !path.exists() {
        return LauncherSettings::default();
    }
    let Ok(raw) = fs::read_to_string(&path) else {
        return LauncherSettings::default();
    };
    if raw.trim().is_empty() {
        return LauncherSettings::default();
    }
    let (mut settings, migrated) = normalize_loaded_settings(&raw);
    let vault_migrated = privacy_vault::hydrate_settings_from_vault(&mut settings);
    if migrated || vault_migrated {
        let scrubbed = privacy_vault::extract_secrets_for_settings_save(&settings);
        if let Ok(serialized) = serde_json::to_string_pretty(&scrubbed) {
            let _ = atomic_write_string(&path, &serialized);
        }
    }
    settings
}

pub(crate) fn save_settings_to_disk(app: &AppHandle, settings: &LauncherSettings) -> Result<(), String> {
    let _guard = get_settings_io_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let path = get_app_data_file(app, "settings.json")?;
    let scrubbed = privacy_vault::extract_secrets_for_settings_save(settings);
    let serialized = serde_json::to_string_pretty(&scrubbed)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    atomic_write_string(&path, &serialized)
}

pub(crate) fn save_settings_standalone(settings: &LauncherSettings) -> Result<(), String> {
    let _guard = get_settings_io_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let path = get_app_data_file_standalone("settings.json")?;
    let scrubbed = privacy_vault::extract_secrets_for_settings_save(settings);
    let serialized = serde_json::to_string_pretty(&scrubbed)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    atomic_write_string(&path, &serialized)
}

#[tauri::command]
pub(crate) async fn get_launcher_settings(app: AppHandle) -> Result<LauncherSettings, String> {
    let app_clone = app.clone();
    tokio::task::spawn_blocking(move || load_settings_from_disk(&app_clone).to_ipc_safe())
        .await
        .map_err(|e| format!("Failed to read settings: {}", e))
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub(crate) async fn update_launcher_settings(
    app: AppHandle,
    auto_clean_completed: Option<bool>,
    steamgriddb_api_key: Option<String>,
    suspend_hotkey: Option<String>,
    couch_mode: Option<bool>,
    auto_detect_clipboard: Option<bool>,
    nvme_prefetch_enabled: Option<bool>,
    swarm_enabled: Option<bool>,
    swarm_node_name: Option<String>,
    swarm_peers: Option<Vec<String>>,
    steam_api_key: Option<String>,
    clear_steam_api_key: Option<bool>,
    steam_id_override: Option<String>,
    xbox_api_key: Option<String>,
    clear_xbox_api_key: Option<bool>,
    psn_npsso: Option<String>,
    clear_psn_npsso: Option<bool>,
    default_save_dir: Option<String>,
    download_speed_preset: Option<String>,
    compact_library_cards: Option<bool>,
    performance_boost_enabled: Option<bool>,
    auto_backup_saves_on_exit: Option<bool>,
    cpu_priority_mode: Option<String>,
    ram_trim_mode: Option<String>,
    auto_freeze_minimized: Option<bool>,
    nvme_cache_budget_pct: Option<u8>,
    doh_enabled: Option<bool>,
    custom_proxy_url: Option<String>,
    custom_speed_cap_mbps: Option<f64>,
    auto_unblock_motw: Option<bool>,
    portable_sandbox_default: Option<bool>,
    max_restore_points: Option<u32>,
    block_game_telemetry: Option<bool>,
    global_launch_args: Option<String>,
    swarm_auto_sync_saves: Option<bool>,
    start_minimized_with_windows: Option<bool>,
    theme: Option<String>,
    card_zoom_px: Option<u32>,
    projfs_streaming_enabled: Option<bool>,
    shader_swarm_enabled: Option<bool>,
    frame_pacing_enabled: Option<bool>,
    disk_pressure_threshold_gb: Option<f64>,
    disk_pressure_days_threshold: Option<u32>,
    discord_rpc_enabled: Option<bool>,
    realtime_save_watcher_enabled: Option<bool>,
    desktop_chord_enabled: Option<bool>,
    minimize_to_tray: Option<bool>,
    close_to_tray: Option<bool>,
    auto_check_updates: Option<bool>,
) -> Result<LauncherSettings, String> {
    let app_for_save = app.clone();
    let autostart_change = start_minimized_with_windows;
    let updated = tokio::task::spawn_blocking(move || -> Result<LauncherSettings, String> {
        let mut current = load_settings_from_disk(&app_for_save);
        if let Some(flag) = auto_clean_completed {
            current.auto_clean_completed = flag;
        }
        if let Some(key) = steamgriddb_api_key {
            let trimmed = key.trim().to_string();
            if trimmed.is_empty() {
                current.steamgriddb_api_key = None;
                privacy_vault::clear_slot_by_name("steamgriddb");
            } else {
                current.steamgriddb_api_key = Some(trimmed);
            }
        }
        if let Some(hk) = suspend_hotkey {
            let trimmed = hk.trim().to_string();
            if !trimmed.is_empty() && duke_core::win32_ffi::parse_hotkey_spec(&trimmed).is_some() {
                current.suspend_hotkey = trimmed;
            }
        }
        if let Some(couch) = couch_mode {
            current.couch_mode = couch;
        }
        if let Some(detect) = auto_detect_clipboard {
            current.auto_detect_clipboard = detect;
        }
        if let Some(prefetch) = nvme_prefetch_enabled {
            current.nvme_prefetch_enabled = prefetch;
        }
        if let Some(swarm) = swarm_enabled {
            current.swarm_enabled = swarm;
        }
        if let Some(node_name) = swarm_node_name {
            current.swarm_node_name = node_name.trim().to_string();
        }
        if let Some(peers) = swarm_peers {
            current.swarm_peers = peers
                .into_iter()
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect();
        }
        if clear_steam_api_key.unwrap_or(false) {
            current.steam_api_key = None;
            privacy_vault::clear_slot_by_name("steam");
        } else if let Some(key) = steam_api_key {
            let trimmed = key.trim().to_string();
            if !trimmed.is_empty() {
                current.steam_api_key = Some(trimmed);
            }
        }
        if let Some(sid) = steam_id_override {
            let trimmed = sid.trim().to_string();
            current.steam_id_override = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            };
        }
        if clear_xbox_api_key.unwrap_or(false) {
            current.xbox_api_key = None;
            privacy_vault::clear_slot_by_name("xbox");
        } else if let Some(key) = xbox_api_key {
            let trimmed = key.trim().to_string();
            if !trimmed.is_empty() {
                current.xbox_api_key = Some(trimmed);
            }
        }
        if clear_psn_npsso.unwrap_or(false) {
            current.psn_npsso = None;
            privacy_vault::clear_slot_by_name("psn");
        } else if let Some(tok) = psn_npsso {
            let trimmed = tok.trim().to_string();
            if !trimmed.is_empty() {
                current.psn_npsso = Some(trimmed);
            }
        }
        if let Some(dir) = default_save_dir {
            let trimmed = dir.trim().to_string();
            current.default_save_dir = if trimmed.is_empty() {
                None
            } else {
                let path = PathBuf::from(&trimmed);
                if !path.exists() {
                    fs::create_dir_all(&path)
                        .map_err(|e| format!("Could not create save folder '{}': {}", trimmed, e))?;
                }
                Some(trimmed)
            };
        }
        if let Some(preset) = download_speed_preset {
            let norm = preset.trim().to_ascii_lowercase();
            current.download_speed_preset = match norm.as_str() {
                "balanced" | "background" | "max" => norm,
                _ => "max".to_string(),
            };
        }
        if let Some(compact) = compact_library_cards {
            current.compact_library_cards = compact;
        }
        if let Some(boost) = performance_boost_enabled {
            current.performance_boost_enabled = boost;
            current.nvme_prefetch_enabled = boost && current.nvme_cache_budget_pct > 0;
        }
        if let Some(auto_bak) = auto_backup_saves_on_exit {
            current.auto_backup_saves_on_exit = auto_bak;
        }
        if let Some(cpu_mode) = cpu_priority_mode {
            let norm = cpu_mode.trim().to_ascii_lowercase();
            current.cpu_priority_mode = match norm.as_str() {
                "max_performance" | "quiet" | "auto" => norm,
                _ => "auto".to_string(),
            };
        }
        if let Some(trim_mode) = ram_trim_mode {
            let norm = trim_mode.trim().to_ascii_lowercase();
            current.ram_trim_mode = match norm.as_str() {
                "off" | "aggressive" | "standard" => norm,
                _ => "standard".to_string(),
            };
        }
        if let Some(freeze) = auto_freeze_minimized {
            current.auto_freeze_minimized = freeze;
        }
        if let Some(pct) = nvme_cache_budget_pct {
            current.nvme_cache_budget_pct = pct.min(40);
            current.nvme_prefetch_enabled = current.nvme_cache_budget_pct > 0;
        }
        if let Some(doh) = doh_enabled {
            current.doh_enabled = doh;
        }
        if let Some(proxy) = custom_proxy_url {
            current.custom_proxy_url = duke_core::net_doh::validate_proxy_url(Some(&proxy))?;
        }
        if let Some(cap) = custom_speed_cap_mbps {
            current.custom_speed_cap_mbps = if cap.is_finite() && cap > 0.0 {
                cap.min(10_000.0)
            } else {
                0.0
            };
        }
        if let Some(motw) = auto_unblock_motw {
            current.auto_unblock_motw = motw;
        }
        if let Some(sandbox) = portable_sandbox_default {
            current.portable_sandbox_default = sandbox;
        }
        if let Some(max_pts) = max_restore_points {
            current.max_restore_points = max_pts.clamp(1, 50);
        }
        if let Some(block_tel) = block_game_telemetry {
            current.block_game_telemetry = block_tel;
        }
        if let Some(args) = global_launch_args {
            let trimmed = args.trim().to_string();
            current.global_launch_args = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            };
        }
        if let Some(auto_sync) = swarm_auto_sync_saves {
            current.swarm_auto_sync_saves = auto_sync;
        }
        if let Some(start_min) = autostart_change {
            win32_clipboard_protocol::set_windows_autostart_minimized(start_min)?;
            current.start_minimized_with_windows = start_min;
        }
        if let Some(next_theme) = theme {
            current.theme = normalize_theme_id(&next_theme);
        }
        if let Some(zoom_px) = card_zoom_px {
            current.card_zoom_px = zoom_px.clamp(125, 280);
        }
        if let Some(pfs) = projfs_streaming_enabled {
            current.projfs_streaming_enabled = pfs;
        }
        if let Some(shd) = shader_swarm_enabled {
            current.shader_swarm_enabled = shd;
        }
        if let Some(fp) = frame_pacing_enabled {
            current.frame_pacing_enabled = fp;
        }
        if let Some(thresh_gb) = disk_pressure_threshold_gb {
            current.disk_pressure_threshold_gb = thresh_gb.clamp(5.0, 1000.0);
        }
        if let Some(days) = disk_pressure_days_threshold {
            current.disk_pressure_days_threshold = days.clamp(7, 3650);
        }
        if let Some(rpc) = discord_rpc_enabled {
            current.discord_rpc_enabled = rpc;
        }
        if let Some(watcher) = realtime_save_watcher_enabled {
            current.realtime_save_watcher_enabled = watcher;
        }
        if let Some(chord) = desktop_chord_enabled {
            current.desktop_chord_enabled = chord;
        }
        if let Some(min_tray) = minimize_to_tray {
            current.minimize_to_tray = min_tray;
        }
        if let Some(close_tray) = close_to_tray {
            current.close_to_tray = close_tray;
        }
        if let Some(auto_chk) = auto_check_updates {
            current.auto_check_updates = auto_chk;
        }

        save_settings_to_disk(&app_for_save, &current)?;
        apply_runtime_settings(&current);
        Ok(current.to_ipc_safe())
    })
    .await
    .map_err(|e| format!("Failed to save settings: {}", e))??;

    if updated.swarm_enabled {
        swarm::start_swarm_background_services();
    }

    if updated.auto_clean_completed {
        let remaining_opt = {
            let mut guard = get_queue_store().lock().unwrap_or_else(|e| e.into_inner());
            if prune_auto_cleanable_jobs(&mut guard, true) {
                persist_queue_to_disk(&app, &guard);
                Some(guard.clone())
            } else {
                None
            }
        };
        if let Some(remaining) = remaining_opt {
            let _ = app.emit("download-queue-updated", &remaining);
        }
    }

    Ok(updated)
}

pub(crate) static PENDING_PROTOCOL_URLS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

pub(crate) fn get_pending_protocol_urls() -> &'static Mutex<Vec<String>> {
    PENDING_PROTOCOL_URLS.get_or_init(|| Mutex::new(Vec::new()))
}

pub(crate) fn parse_duke_protocol_download_url(raw_arg: &str) -> Option<String> {
    let cleaned = raw_arg.trim().trim_matches('"').trim();
    if cleaned.is_empty() {
        return None;
    }
    let parsed = Url::parse(cleaned).ok()?;
    if !parsed.scheme().eq_ignore_ascii_case("duke") {
        return None;
    }
    let is_download_route = parsed
        .host_str()
        .map(|h| h.eq_ignore_ascii_case("download"))
        .unwrap_or_else(|| parsed.path().trim_matches('/').eq_ignore_ascii_case("download"));
    if !is_download_route {
        return None;
    }
    let raw_query = parsed.query()?;
    let encoded_target = raw_query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(k, _)| k.eq_ignore_ascii_case("url"))
        .map(|(_, v)| v)?;
    let decoded = urlencoding::decode(encoded_target).ok()?;
    let validated = validate_external_http_url(&decoded).ok()?;
    Some(validated.to_string())
}

pub(crate) fn focus_main_window(app: &AppHandle) {
    if let Some(daemon) = TRAY_DAEMON.get() {
        daemon.ui_visible.store(true, Ordering::Release);
    }
    duke_core::tray_daemon::reopen_main_window(app, "main");
}

pub(crate) fn enqueue_protocol_download_url(
    app: AppHandle,
    download_url: String,
    focus_window: bool,
    record_pending: bool,
) {
    let Ok(validated) = validate_external_http_url(&download_url) else {
        return;
    };
    let clean_url = validated.to_string();
    if focus_window {
        focus_main_window(&app);
    }
    if record_pending {
        let mut guard = get_pending_protocol_urls()
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if !guard.iter().any(|existing| existing == &clean_url) {
            guard.push(clean_url.clone());
        }
    }
    let _ = app.emit(
        "duke-uri-download",
        serde_json::json!({ "url": clean_url.clone() }),
    );
    tauri::async_runtime::spawn(async move {
        let _ = enqueue_download_internal(
            app,
            clean_url,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .await;
    });
}

#[cfg(target_os = "windows")]
pub(crate) mod win32_clipboard_protocol {
    use std::ffi::c_void;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::sync::OnceLock;
    use std::time::Duration;
    use tauri::AppHandle;

    const HKEY_CURRENT_USER: isize = -2147483647isize;
    const KEY_WRITE: u32 = 0x20006;
    const REG_SZ: u32 = 1;
    const CF_UNICODETEXT: u32 = 13;
    const GMEM_MOVEABLE: u32 = 0x0002;
    const PIPE_ACCESS_INBOUND: u32 = 0x00000001;
    const PIPE_TYPE_BYTE: u32 = 0x00000000;
    const PIPE_READMODE_BYTE: u32 = 0x00000000;
    const PIPE_WAIT: u32 = 0x00000000;
    const PIPE_UNLIMITED_INSTANCES: u32 = 255;
    const INVALID_HANDLE_VALUE: isize = -1;
    const ERROR_ALREADY_EXISTS: u32 = 183;
    const ERROR_PIPE_CONNECTED: u32 = 535;
    const DUKE_SINGLE_INSTANCE_MUTEX: &str = "Local\\DukeLauncherSingleInstanceMutex";
    const DUKE_PROTOCOL_PIPE_NAME: &str = r"\\.\pipe\duke-launcher-uri-ipc";

    static SINGLE_INSTANCE_MUTEX_HANDLE: OnceLock<isize> = OnceLock::new();

    #[link(name = "user32")]
    extern "system" {
        fn OpenClipboard(hWndNewOwner: isize) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(uFormat: u32) -> isize;
        fn EmptyClipboard() -> i32;
        fn SetClipboardData(uFormat: u32, hMem: isize) -> isize;
        fn IsClipboardFormatAvailable(format: u32) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalLock(hMem: isize) -> *mut c_void;
        fn GlobalUnlock(hMem: isize) -> i32;
        fn GlobalAlloc(uFlags: u32, dwBytes: usize) -> isize;
        fn GlobalFree(hMem: isize) -> isize;
        fn CreateMutexW(
            lpMutexAttributes: *const c_void,
            bInitialOwner: i32,
            lpName: *const u16,
        ) -> isize;
        fn CreateNamedPipeW(
            lpName: *const u16,
            dwOpenMode: u32,
            dwPipeMode: u32,
            nMaxInstances: u32,
            nOutBufferSize: u32,
            nInBufferSize: u32,
            nDefaultTimeOut: u32,
            lpSecurityAttributes: *const c_void,
        ) -> isize;
        fn ConnectNamedPipe(hNamedPipe: isize, lpOverlapped: *mut c_void) -> i32;
        fn DisconnectNamedPipe(hNamedPipe: isize) -> i32;
        fn ReadFile(
            hFile: isize,
            lpBuffer: *mut u8,
            nNumberOfBytesToRead: u32,
            lpNumberOfBytesRead: *mut u32,
            lpOverlapped: *mut c_void,
        ) -> i32;
        fn CloseHandle(hObject: isize) -> i32;
        fn GetLastError() -> u32;
    }

    #[link(name = "advapi32")]
    extern "system" {
        fn RegCreateKeyExW(
            hKey: isize,
            lpSubKey: *const u16,
            Reserved: u32,
            lpClass: *mut u16,
            dwOptions: u32,
            samDesired: u32,
            lpSecurityAttributes: *const c_void,
            phkResult: *mut isize,
            lpdwDisposition: *mut u32,
        ) -> i32;
        fn RegSetValueExW(
            hKey: isize,
            lpValueName: *const u16,
            Reserved: u32,
            dwType: u32,
            lpData: *const u8,
            cbData: u32,
        ) -> i32;
        fn RegDeleteValueW(hKey: isize, lpValueName: *const u16) -> i32;
        fn RegCloseKey(hKey: isize) -> i32;
    }

    struct ClipboardGuard;
    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            unsafe {
                CloseClipboard();
            }
        }
    }

    struct RegKeyGuard(isize);
    impl Drop for RegKeyGuard {
        fn drop(&mut self) {
            if self.0 != 0 {
                unsafe {
                    RegCloseKey(self.0);
                }
            }
        }
    }

    fn to_wide_null(input: &str) -> Vec<u16> {
        input.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn open_clipboard_with_retry() -> Result<ClipboardGuard, String> {
        for _ in 0..6 {
            if unsafe { OpenClipboard(0) } != 0 {
                return Ok(ClipboardGuard);
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        Err("System clipboard is currently locked by another process.".to_string())
    }

    pub fn read_clipboard_text() -> Result<String, String> {
        if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) } == 0 {
            return Ok(String::new());
        }
        let _guard = open_clipboard_with_retry()?;
        let handle = unsafe { GetClipboardData(CF_UNICODETEXT) };
        if handle == 0 {
            return Ok(String::new());
        }
        let ptr = unsafe { GlobalLock(handle) } as *const u16;
        if ptr.is_null() {
            return Ok(String::new());
        }
        let mut len = 0usize;
        const MAX_U16_LEN: usize = 1_048_576;
        unsafe {
            while len < MAX_U16_LEN && *ptr.add(len) != 0 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts(ptr, len);
            let text = String::from_utf16_lossy(slice);
            GlobalUnlock(handle);
            Ok(text)
        }
    }

    pub fn write_clipboard_text(text: &str) -> Result<(), String> {
        let wide = to_wide_null(text);
        let byte_len = wide.len() * std::mem::size_of::<u16>();
        let hmem = unsafe { GlobalAlloc(GMEM_MOVEABLE, byte_len) };
        if hmem == 0 {
            return Err("Failed to allocate clipboard memory.".to_string());
        }
        let dst = unsafe { GlobalLock(hmem) } as *mut u16;
        if dst.is_null() {
            unsafe {
                GlobalFree(hmem);
            }
            return Err("Failed to lock clipboard memory.".to_string());
        }
        unsafe {
            std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
            GlobalUnlock(hmem);
        }
        let _guard = match open_clipboard_with_retry() {
            Ok(guard) => guard,
            Err(e) => {
                unsafe {
                    GlobalFree(hmem);
                }
                return Err(e);
            }
        };
        unsafe {
            EmptyClipboard();
            if SetClipboardData(CF_UNICODETEXT, hmem) == 0 {
                GlobalFree(hmem);
                return Err("Failed to write text to clipboard.".to_string());
            }
        }
        Ok(())
    }

    fn reg_write_hkcu_sz(subkey: &str, value_name: Option<&str>, data: &str) -> Result<(), String> {
        let subkey_w = to_wide_null(subkey);
        let mut hkey: isize = 0;
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                subkey_w.as_ptr(),
                0,
                std::ptr::null_mut(),
                0,
                KEY_WRITE,
                std::ptr::null(),
                &mut hkey,
                std::ptr::null_mut(),
            )
        };
        if status != 0 || hkey == 0 {
            return Err(format!("RegCreateKeyExW({}) failed: {}", subkey, status));
        }
        let _key_guard = RegKeyGuard(hkey);
        let name_w = value_name.map(to_wide_null);
        let name_ptr = name_w
            .as_ref()
            .map_or(std::ptr::null(), |wide| wide.as_ptr());
        let data_w = to_wide_null(data);
        let data_bytes = (data_w.len() * std::mem::size_of::<u16>()) as u32;
        let set_status = unsafe {
            RegSetValueExW(
                hkey,
                name_ptr,
                0,
                REG_SZ,
                data_w.as_ptr() as *const u8,
                data_bytes,
            )
        };
        if set_status != 0 {
            return Err(format!("RegSetValueExW({}) failed: {}", subkey, set_status));
        }
        Ok(())
    }

    pub fn register_duke_uri_protocol() {
        let Ok(exe_path) = std::env::current_exe() else {
            return;
        };
        let exe_str = exe_path.to_string_lossy();
        let command_line = format!("\"{}\" \"%1\"", exe_str);
        let icon_value = format!("\"{}\",0", exe_str);

        let _ = reg_write_hkcu_sz(r"Software\Classes\duke", None, "URL:Duke Launcher Protocol");
        let _ = reg_write_hkcu_sz(r"Software\Classes\duke", Some("URL Protocol"), "");
        let _ = reg_write_hkcu_sz(r"Software\Classes\duke\DefaultIcon", None, &icon_value);
        let _ = reg_write_hkcu_sz(r"Software\Classes\duke\shell\open\command", None, &command_line);
    }

    pub fn set_windows_autostart_minimized(enable: bool) -> Result<(), String> {
        let subkey = r"Software\Microsoft\Windows\CurrentVersion\Run";
        let value_name = "DukeLauncher";
        if enable {
            let exe_path = std::env::current_exe()
                .map_err(|e| format!("Failed resolving executable path: {}", e))?;
            let cmd = format!("\"{}\" --minimized", exe_path.to_string_lossy());
            return reg_write_hkcu_sz(subkey, Some(value_name), &cmd);
        }

        let subkey_w = to_wide_null(subkey);
        let mut hkey: isize = 0;
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                subkey_w.as_ptr(),
                0,
                std::ptr::null_mut(),
                0,
                KEY_WRITE,
                std::ptr::null(),
                &mut hkey,
                std::ptr::null_mut(),
            )
        };
        if status != 0 || hkey == 0 {
            return Ok(());
        }
        let _key_guard = RegKeyGuard(hkey);
        let name_w = to_wide_null(value_name);
        let del_status = unsafe { RegDeleteValueW(hkey, name_w.as_ptr()) };
        if del_status != 0 && del_status != 2 {
            return Err(format!("RegDeleteValueW({}) failed: {}", value_name, del_status));
        }
        Ok(())
    }

    pub fn acquire_single_instance_or_forward(raw_args: &[String]) -> bool {
        let mutex_name = to_wide_null(DUKE_SINGLE_INSTANCE_MUTEX);
        let mutex = unsafe { CreateMutexW(std::ptr::null(), 0, mutex_name.as_ptr()) };
        if mutex == 0 {
            return true;
        }
        let last_err = unsafe { GetLastError() };
        if last_err != ERROR_ALREADY_EXISTS {
            let _ = SINGLE_INSTANCE_MUTEX_HANDLE.set(mutex);
            return true;
        }
        unsafe {
            CloseHandle(mutex);
        }

        let mut payload = String::from("FOCUS\n");
        for arg in raw_args {
            if super::parse_duke_protocol_download_url(arg).is_some() {
                payload.push_str("URI:");
                payload.push_str(arg.trim());
                payload.push('\n');
            }
        }

        for _ in 0..20 {
            if let Ok(mut pipe) = OpenOptions::new().write(true).open(DUKE_PROTOCOL_PIPE_NAME) {
                let _ = pipe.write_all(payload.as_bytes());
                let _ = pipe.flush();
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    pub fn start_protocol_pipe_server(app: AppHandle) {
        std::thread::spawn(move || {
            let pipe_name = to_wide_null(DUKE_PROTOCOL_PIPE_NAME);
            loop {
                let pipe = unsafe {
                    CreateNamedPipeW(
                        pipe_name.as_ptr(),
                        PIPE_ACCESS_INBOUND,
                        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                        PIPE_UNLIMITED_INSTANCES,
                        0,
                        4096,
                        0,
                        std::ptr::null(),
                    )
                };
                if pipe == INVALID_HANDLE_VALUE || pipe == 0 {
                    std::thread::sleep(Duration::from_millis(250));
                    continue;
                }

                let connected = unsafe { ConnectNamedPipe(pipe, std::ptr::null_mut()) };
                let connected_ok =
                    connected != 0 || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
                if !connected_ok {
                    unsafe {
                        CloseHandle(pipe);
                    }
                    continue;
                }

                let mut payload = Vec::with_capacity(1024);
                let mut chunk = [0u8; 1024];
                loop {
                    let mut read_len = 0u32;
                    let ok = unsafe {
                        ReadFile(
                            pipe,
                            chunk.as_mut_ptr(),
                            chunk.len() as u32,
                            &mut read_len,
                            std::ptr::null_mut(),
                        )
                    };
                    if ok == 0 || read_len == 0 {
                        break;
                    }
                    let take = (read_len as usize).min(32768usize.saturating_sub(payload.len()));
                    payload.extend_from_slice(&chunk[..take]);
                    if payload.len() >= 32768 {
                        break;
                    }
                }
                unsafe {
                    DisconnectNamedPipe(pipe);
                    CloseHandle(pipe);
                }

                let text = String::from_utf8_lossy(&payload);
                let mut any_uri = false;
                let mut requested_focus = false;
                for raw_line in text.lines() {
                    let line = raw_line.trim();
                    if line == "FOCUS" {
                        requested_focus = true;
                    } else if let Some(uri) = line.strip_prefix("URI:") {
                        if let Some(download_url) = super::parse_duke_protocol_download_url(uri) {
                            any_uri = true;
                            super::enqueue_protocol_download_url(
                                app.clone(),
                                download_url,
                                true,
                                false,
                            );
                        }
                    }
                }
                if requested_focus && !any_uri {
                    super::focus_main_window(&app);
                }
            }
        });
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) mod win32_clipboard_protocol {
    use tauri::AppHandle;

    pub fn read_clipboard_text() -> Result<String, String> {
        Ok(String::new())
    }

    pub fn write_clipboard_text(_text: &str) -> Result<(), String> {
        Err("Clipboard write is only supported on Windows.".to_string())
    }

    pub fn register_duke_uri_protocol() {}

    pub fn set_windows_autostart_minimized(_enable: bool) -> Result<(), String> {
        Ok(())
    }

    pub fn acquire_single_instance_or_forward(_raw_args: &[String]) -> bool {
        true
    }

    pub fn start_protocol_pipe_server(_app: AppHandle) {}
}

#[tauri::command]
pub(crate) async fn read_clipboard_text() -> Result<String, String> {
    run_blocking_timeout(5, "read_clipboard_text", || {
        win32_clipboard_protocol::read_clipboard_text()
    })
    .await
}

#[tauri::command]
pub(crate) async fn write_clipboard_text(text: String) -> Result<(), String> {
    run_blocking_timeout(5, "write_clipboard_text", move || {
        win32_clipboard_protocol::write_clipboard_text(&text)
    })
    .await
}

#[tauri::command]
pub(crate) async fn take_pending_protocol_urls() -> Result<Vec<String>, String> {
    run_blocking_timeout(5, "take_pending_protocol_urls", || {
        let mut guard = get_pending_protocol_urls()
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        Ok(std::mem::take(&mut *guard))
    })
    .await
}

pub(crate) async fn launch_game_internal(
    app: AppHandle,
    window: Option<WebviewWindow>,
    exe_path: String,
    game_id: Option<String>,
    explicit_launch_args: Option<String>,
) -> Result<(), String> {
    let _launch_crumb = self_heal::record_heavy_command_breadcrumb("launch_game");
    let trimmed = exe_path.trim().trim_matches('"').to_string();
    let exe_file = PathBuf::from(&trimmed);

    let start_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let app_for_start = app.clone();
    let path_for_start = trimmed.clone();
    let gid_for_start = game_id.clone();
    let explicit_args_clone = explicit_launch_args
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let fallback_game_title = exe_file
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Game".to_string());
    let default_root_dir = if exe_file.is_file() {
        resolve_game_root_dir(&exe_file)
    } else {
        exe_file
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(PathBuf::new)
    };
    let default_root_fallback = default_root_dir.clone();

    struct LaunchContext {
        saved_args: Option<String>,
        resolved_title: String,
        sb_enabled: bool,
        pf_enabled: bool,
        g_root: PathBuf,
        plat_id: u16,
        app_id: u32,
        emu_id: Option<String>,
        rom_from_lib: Option<String>,
        global_settings: LauncherSettings,
        matched_entry: Option<GameEntry>,
    }

    let launch_ctx = tokio::task::spawn_blocking(move || {
        let mut saved_args = explicit_args_clone.clone();
        let mut resolved_title = fallback_game_title;
        let global_settings = load_settings_from_disk(&app_for_start);
        let mut sb_enabled = global_settings.portable_sandbox_default;
        let mut pf_enabled = global_settings.performance_boost_enabled
            && global_settings.nvme_prefetch_enabled
            && global_settings.nvme_cache_budget_pct > 0;
        let mut g_root = default_root_dir;
        let mut plat_id = 0u16;
        let mut app_id = 0u32;
        let mut emu_id: Option<String> = None;
        let mut rom_from_lib: Option<String> = None;
        let mut matched_entry: Option<GameEntry> = None;
        if let Ok(mut library) = load_library_from_disk(&app_for_start) {
            if let Some(item) = library.iter_mut().find(|g| {
                gid_for_start
                    .as_ref()
                    .map(|id| g.id == *id)
                    .unwrap_or(false)
                    || (!path_for_start.is_empty() && g.exe_path.eq_ignore_ascii_case(&path_for_start))
            }) {
                if !item.title.trim().is_empty() {
                    resolved_title = item.title.trim().to_string();
                }
                plat_id = item.platform_id;
                app_id = item
                    .app_id
                    .or_else(|| item.steam_app_id.and_then(|v| u32::try_from(v).ok()))
                    .unwrap_or(0);
                if !item.emulator_id.trim().is_empty() {
                    emu_id = Some(item.emulator_id.trim().to_string());
                }
                if path_for_start.is_empty() && !item.exe_path.trim().is_empty() {
                    rom_from_lib = Some(item.exe_path.trim().to_string());
                }
                sb_enabled = item.sandbox_enabled || global_settings.portable_sandbox_default;
                let item_pf = item
                    .nvme_prefetch_enabled
                    .unwrap_or(global_settings.nvme_prefetch_enabled);
                pf_enabled = global_settings.performance_boost_enabled
                    && item_pf
                    && global_settings.nvme_cache_budget_pct > 0;
                if !item.install_dir.trim().is_empty() {
                    let cand = PathBuf::from(item.install_dir.trim());
                    if cand.is_dir() {
                        g_root = cand;
                    }
                }
                if !path_for_start.is_empty() || rom_from_lib.is_some() {
                    item.last_played = Some(start_ts);
                    item.last_played_ts = Some(start_ts);
                }
                if saved_args.is_none() {
                    saved_args = item.launch_args.clone();
                } else {
                    item.launch_args = saved_args.clone();
                }
                matched_entry = Some(item.clone());
                let _ = save_library_to_disk(&app_for_start, &library);
                let _ = app_for_start.emit("library-updated", &library);
            }
        }
        LaunchContext {
            saved_args,
            resolved_title,
            sb_enabled,
            pf_enabled,
            g_root,
            plat_id,
            app_id,
            emu_id,
            rom_from_lib,
            global_settings,
            matched_entry,
        }
    })
    .await
    .unwrap_or_else(|_| {
        let fallback = load_settings_standalone();
        LaunchContext {
            saved_args: None,
            resolved_title: String::from("Game"),
            sb_enabled: fallback.portable_sandbox_default,
            pf_enabled: fallback.performance_boost_enabled
                && fallback.nvme_prefetch_enabled
                && fallback.nvme_cache_budget_pct > 0,
            g_root: default_root_fallback,
            plat_id: 0,
            app_id: 0,
            emu_id: None,
            rom_from_lib: None,
            global_settings: fallback,
            matched_entry: None,
        }
    });

    let resolved_cli_args = launch_ctx.saved_args;
    let game_title = launch_ctx.resolved_title;
    let sandbox_enabled = launch_ctx.sb_enabled;
    let prefetch_enabled = launch_ctx.pf_enabled;
    let game_root_dir = launch_ctx.g_root;
    let item_platform_id = launch_ctx.plat_id;
    let item_app_id = launch_ctx.app_id;
    let item_emulator_id = launch_ctx.emu_id;
    let resolved_rom_path_from_lib = launch_ctx.rom_from_lib;
    let launch_settings = launch_ctx.global_settings;
    let matched_entry_opt = launch_ctx.matched_entry;

    let trimmed = if trimmed.is_empty() {
        resolved_rom_path_from_lib.unwrap_or_default()
    } else {
        trimmed
    };
    let exe_file = PathBuf::from(&trimmed);

    if item_platform_id > 0 && (trimmed.is_empty() || !exe_file.exists() || !exe_file.is_file()) {
        return Err(format!(
            "ROM_NOT_LINKED|{}|{}",
            game_id.clone().unwrap_or_default(),
            game_title
        ));
    }

    if !exe_file.exists() || !exe_file.is_file() {
        return Err(format!("Game executable not found at: {}", trimmed));
    }

    let (spawn_exe, working_dir, emu_prefix_args, resolved_emu_id) = if item_platform_id > 0 {
        let resolved = emulators::resolve_emulator_launch(
            item_platform_id,
            item_emulator_id.as_deref(),
            &exe_file,
        )?;
        unblock_ntfs_zone_identifier(&exe_file);
        unblock_ntfs_zone_identifier(&resolved.exe_path);
        (
            resolved.exe_path,
            resolved.working_dir,
            resolved.args,
            resolved.emulator_id,
        )
    } else {
        let q_dir = if cfg!(test) {
            None
        } else {
            pe_inspect::quarantine_root_dir_standalone().ok()
        };
        if let Ok(shield) = pe_inspect::inspect_pe_security_shield_file(&exe_file, false, None) {
            if shield.shield_badge == "Blocked" {
                let reasons: Vec<String> = shield
                    .suspicious_behaviors
                    .iter()
                    .map(|f| format!("{}: {}", f.title, f.explanation))
                    .collect();
                let q_note = q_dir
                    .as_deref()
                    .and_then(|qd| pe_inspect::quarantine_high_risk_binary(&exe_file, qd, &reasons, None).ok())
                    .map(|p| format!(" Quarantined to {}", p.display()))
                    .unwrap_or_default();
                return Err(format!(
                    "Security Shield v2 blocked pre-launch execution of '{}': {}.{}",
                    game_title,
                    reasons.join(" | "),
                    q_note
                ));
            }
        }
        // Automatically strip NTFS Mark-of-the-Web (:Zone.Identifier) ONLY after passing Security Shield v2
        unblock_ntfs_zone_identifier(&exe_file);
        if let Some(ref gid_v) = game_id {
            if let Ok(droot) = resolve_data_root_dir_standalone() {
                if let Some(applied_msg) = duke_verified::auto_apply_verified_wrappers_on_first_launch(
                    gid_v,
                    &exe_file,
                    &game_root_dir,
                    &droot.join("wrappers"),
                ) {
                    let _ = app.emit(
                        "duke-verified-applied",
                        serde_json::json!({ "gameId": gid_v, "message": applied_msg }),
                    );
                }
            }
        }
        let wd = exe_file
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| "Unable to determine executable working directory.".to_string())?;
        (exe_file.clone(), wd, Vec::new(), String::new())
    };

    let exe_name_for_bus = spawn_exe
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();

    if prefetch_enabled && item_platform_id > 0 {
        let rom_len = fs::metadata(&exe_file).map(|m| m.len()).unwrap_or(0);
        if rom_len > 32 * 1024 * 1024 {
            let warm_rom = exe_file.clone();
            let warm_budget_pct = launch_settings.nvme_cache_budget_pct.clamp(1, 40);
            if let Ok(Ok(report)) = tokio::task::spawn_blocking(move || {
                duke_core::nvme_prefetch::warm_single_file_with_budget_pct(
                    &warm_rom,
                    warm_budget_pct,
                )
            })
            .await
            {
                let _ = app.emit(
                    "nvme-prefetch-report",
                    serde_json::json!({
                        "gameId": game_id.clone().unwrap_or_default(),
                        "gameTitle": game_title,
                        "filesWarmed": report.files_warmed,
                        "warmedMb": report.warmed_mb,
                        "throughputGbS": report.throughput_gbps,
                        "elapsedMs": report.elapsed_ms,
                        "ramBudgetMb": report.budget_bytes / (1024 * 1024),
                        "usedIoringApi": report.used_ioring_api,
                        "windowsBuild": report.windows_build
                    }),
                );
            }
        }
    }

    let session_launch_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if launch_settings.shader_swarm_enabled && launch_settings.swarm_enabled {
        let gid_shader = game_id.clone().unwrap_or_else(|| trimmed.clone());
        if let Ok(sync_rep) = shader_swarm::pull_and_inject_peer_shaders(
            &gid_shader,
            &game_title,
            &exe_file,
            &game_root_dir,
        )
        .await
        {
            if sync_rep.injected_files > 0 || sync_rep.driver_purged_files > 0 {
                let _ = app.emit("shader-swarm-report", &sync_rep);
            }
        }
    }

    if launch_settings.frame_pacing_enabled {
        let fp = duke_core::win32_ffi::begin_frame_pacing_runtime();
        let _ = app.emit("frame-pacing-state", &fp);
    }

    let mut parsed_args = if item_platform_id > 0 {
        emu_prefix_args
    } else {
        launch_settings
            .global_launch_args
            .as_deref()
            .map(parse_cli_launch_args)
            .unwrap_or_default()
    };
    if let Some(per_game_args) = resolved_cli_args.as_deref() {
        parsed_args.extend(parse_cli_launch_args(per_game_args));
    }

    // Anti-Telemetry Game Launch Scrubbing (injected on every launch) + Global/Per-Game CLI Launch Arguments
    let jail_policy = game_id
        .as_deref()
        .map(duke_jail::get_policy_for_game)
        .unwrap_or_default();
    let use_appcontainer_jail = item_platform_id == 0 && jail_policy.jail_enabled;
    if item_platform_id == 0 && !use_appcontainer_jail && jail_policy.offline_mode {
        let _ = duke_jail::apply_or_remove_offline_firewall_rule(&spawn_exe, true);
    }

    let mut launch_env: Vec<(String, String)> = vec![
        ("STEAM_FRAME_FORCE_CLOSE".to_string(), "1".to_string()),
    ];
    for (k, v) in privacy_vault::anti_telemetry_env_pairs() {
        launch_env.push(((*k).to_string(), (*v).to_string()));
    }

    if sandbox_enabled || use_appcontainer_jail {
        if let Ok(env_overrides) = pe_inspect::prepare_game_sandbox_env(&game_root_dir) {
            launch_env.extend(env_overrides);
        }
    }

    if let Some(ref stage_gid) = game_id {
        if let Some(stage_msg) = vhdx_stage::ensure_game_hot_staged_before_launch(stage_gid, &game_root_dir) {
            let _ = app.emit(
                "vhdx-hot-tier-staged",
                serde_json::json!({ "gameId": stage_gid, "message": stage_msg }),
            );
        }
    }
    latency_lab::apply_saved_profile_if_present(&trimmed);
    launch_env.extend(latency_lab::shader_cache_launch_env());

    enum SpawnedChildKind {
        Tokio(Box<tokio::process::Child>),
        JailedProcHandle(isize),
    }

    let mut jailed_thread_to_resume: isize = 0;
    let (mut spawned_child_opt, game_pid) = if use_appcontainer_jail {
        let jail_gid = game_id.as_deref().unwrap_or(&trimmed);
        let profile_dir = game_root_dir.join(".duke-profile");
        let outcome = duke_jail::spawn_jailed_process(
            jail_gid,
            &game_title,
            &spawn_exe,
            &game_root_dir,
            &working_dir,
            &profile_dir,
            &parsed_args,
            &launch_env,
            jail_policy.offline_mode,
        )
        .map_err(|e| {
            restore_normal_priority_mode();
            if let Some(ref w) = window {
                let _ = w.show();
                let _ = w.unminimize();
            }
            let _ = app.emit(
                "game-running-state",
                serde_json::json!({ "running": false, "exePath": trimmed }),
            );
            let _ = app.emit(
                "game-session-state",
                serde_json::json!({ "running": false, "exePath": trimmed }),
            );
            format!("Failed launching inside AppContainer Jail: {}", e)
        })?;
        jailed_thread_to_resume = outcome.thread_handle_raw;
        (
            Some(SpawnedChildKind::JailedProcHandle(outcome.process_handle_raw)),
            outcome.pid,
        )
    } else {
        let mut cmd = tokio::process::Command::new(&spawn_exe);
        cmd.current_dir(&working_dir).args(&parsed_args);
        for (k, v) in &launch_env {
            cmd.env(k, v);
        }
        let child = cmd.spawn().map_err(|e| {
            restore_normal_priority_mode();
            if let Some(ref w) = window {
                let _ = w.show();
                let _ = w.unminimize();
            }
            let _ = app.emit(
                "game-running-state",
                serde_json::json!({ "running": false, "exePath": trimmed }),
            );
            let _ = app.emit(
                "game-session-state",
                serde_json::json!({ "running": false, "exePath": trimmed }),
            );
            format!("Failed to launch '{}': {}", trimmed, e)
        })?;
        let pid = child.id().unwrap_or(0);
        (Some(SpawnedChildKind::Tokio(Box::new(child))), pid)
    };

    let session_key = game_id
        .clone()
        .unwrap_or_else(|| format!("pid_{}", game_pid));
    let job_handle_opt: Option<Arc<duke_core::win32_ffi::ProcessTreeJob>> = if game_pid > 0 {
        duke_core::win32_ffi::create_process_tree_job(game_pid)
            .ok()
            .map(Arc::new)
    } else {
        None
    };

    if jailed_thread_to_resume != 0 {
        duke_jail::resume_thread_keep_process(jailed_thread_to_resume);
    }

    let cpu_mode_for_tree = launch_settings.cpu_priority_mode.clone();
    let should_boost_tree = item_platform_id > 0
        || launch_settings.performance_boost_enabled
        || cpu_mode_for_tree != "auto";
    if game_pid > 0 {
        let mut guard = get_active_sessions().lock().unwrap_or_else(|e| e.into_inner());
        guard.insert(
            session_key.clone(),
            ActiveGameSession {
                game_id: session_key.clone(),
                title: game_title.clone(),
                exe_path: trimmed.clone(),
                pid: game_pid,
                is_suspended: false,
            },
        );
        quick_resume::register_launched_slot(
            &session_key,
            &game_title,
            &trimmed,
            &game_root_dir.to_string_lossy(),
            game_pid,
            item_platform_id as u32,
        );
        let _ = app.emit("quick-resume-updated", quick_resume::build_summary_snapshot());
        if let Some(ref job_arc) = job_handle_opt {
            if let Ok(mut jguard) = get_active_jobs_map().lock() {
                jguard.insert(session_key.clone(), Arc::clone(job_arc));
            }
            let pids = job_arc.active_pids();
            if should_boost_tree {
                if pids.is_empty() {
                    duke_core::win32_ffi::apply_cpu_priority_mode(&[game_pid], &cpu_mode_for_tree);
                } else {
                    duke_core::win32_ffi::apply_cpu_priority_mode(&pids, &cpu_mode_for_tree);
                }
            }
        } else if should_boost_tree {
            duke_core::win32_ffi::apply_cpu_priority_mode(&[game_pid], &cpu_mode_for_tree);
        }
    }
    spawn_hotkey_watcher_if_needed(app.clone());
    if let Some(ref matched) = matched_entry_opt {
        achievements::start_achievement_watcher(app.clone(), matched);
        time_machine::start_save_watcher(app.clone(), matched);
        if launch_settings.realtime_save_watcher_enabled {
            let _ = save_watcher::start_save_watcher_for_game(Some(app.clone()), matched);
        }
    }
    if launch_settings.discord_rpc_enabled {
        let platform_label = if item_platform_id > 0 {
            platforms::platform_by_id(item_platform_id)
                .display_name
                .to_string()
        } else {
            "PC Windows".to_string()
        };
        let coop_status = if duke_link::get_duke_link_status().active {
            Some("Co-op Active")
        } else {
            None
        };
        discord_ipc::set_discord_presence_game(
            &game_title,
            &platform_label,
            coop_status,
            Some(start_ts),
        );
    }

    if game_pid > 0 {
        let emu_hint = if resolved_emu_id.is_empty() {
            item_emulator_id.clone()
        } else {
            Some(resolved_emu_id.clone())
        };
        etw_xray::start_session_xray(
            &app,
            &session_key,
            &game_title,
            game_pid,
            job_handle_opt.clone(),
        );
        duke_upscale::spawn_window_orchestrator(
            session_key.clone(),
            trimmed.clone(),
            emu_hint,
            game_pid,
            job_handle_opt.clone(),
        );
    }

    let launch_exe_hash = duke_core::shm_bus::fnv1a_hash64(trimmed.to_ascii_lowercase().as_bytes());
    let shm_tag = if !game_title.trim().is_empty() {
        game_title.trim()
    } else {
        exe_name_for_bus.trim()
    };
    publish_pipe_event(DukeEvent::GameLaunched {
        title: game_title.clone(),
        exe_name: exe_name_for_bus.clone(),
        pid: game_pid,
        app_id: item_app_id,
        platform_id: item_platform_id,
        exe_hash: launch_exe_hash,
    });
    publish_shm_frame_direct(duke_core::shm_bus::ShmEventFrame::game_launched(
        ShmSourceApp::DukeLauncher,
        game_pid,
        item_app_id,
        item_platform_id,
        sandbox_enabled,
        launch_exe_hash,
        shm_tag,
    ));

    if prefetch_enabled && item_platform_id == 0 && game_root_dir.is_dir() {
        let app_warm = app.clone();
        let warm_dir = game_root_dir.clone();
        let warm_gid = session_key.clone();
        let warm_title = game_title.clone();
        let warm_budget_pct = launch_settings.nvme_cache_budget_pct.clamp(1, 40);
        tokio::spawn(async move {
            if let Ok(Ok(report)) = tokio::task::spawn_blocking(move || {
                duke_core::nvme_prefetch::warm_game_assets_with_budget_pct(&warm_dir, warm_budget_pct)
            })
            .await
            {
                let _ = app_warm.emit(
                    "nvme-prefetch-report",
                    serde_json::json!({
                        "gameId": warm_gid,
                        "gameTitle": warm_title,
                        "filesWarmed": report.files_warmed,
                        "warmedMb": report.warmed_mb,
                        "throughputGbS": report.throughput_gbps,
                        "elapsedMs": report.elapsed_ms,
                        "ramBudgetMb": report.budget_bytes / (1024 * 1024),
                        "usedIoringApi": report.used_ioring_api,
                        "windowsBuild": report.windows_build
                    }),
                );
            }
        });
    }

    let _ = app.emit(
        "game-running-state",
        serde_json::json!({
            "running": true,
            "gameId": session_key,
            "pid": game_pid,
            "isSuspended": false,
            "exePath": trimmed,
            "gameBoost": launch_settings.performance_boost_enabled
        }),
    );
    let _ = app.emit(
        "game-session-state",
        serde_json::json!({
            "running": true,
            "gameId": session_key,
            "pid": game_pid,
            "isSuspended": false,
            "exePath": trimmed,
            "gameBoost": launch_settings.performance_boost_enabled
        }),
    );
    if let Some(ref w) = window {
        let _ = w.minimize();
        let _ = w.hide();
    }
    if launch_settings.performance_boost_enabled {
        enable_ecoqos_game_boost_mode();
    } else if item_platform_id > 0 {
        trim_process_working_set();
    }
    let _ = app.emit(
        "memory-telemetry",
        serde_json::json!({
            "workingSetMb": get_process_working_set_mb(),
            "dohShield": "ACTIVE",
            "status": if launch_settings.performance_boost_enabled { "GAME BOOST" } else { "GAME RUNNING" }
        }),
    );

    let app_after = app.clone();
    let path_after = trimmed.clone();
    let gid_after = game_id;
    let session_key_after = session_key;
    let game_title_after = game_title;
    let exe_name_after = exe_name_for_bus;
    let cpu_mode_loop = launch_settings.cpu_priority_mode.clone();
    let swarm_enabled_exit = launch_settings.swarm_enabled;
    let swarm_auto_sync = launch_settings.swarm_enabled && launch_settings.swarm_auto_sync_saves;
    let shader_swarm_enabled_exit = launch_settings.shader_swarm_enabled;
    let frame_pacing_enabled_exit = launch_settings.frame_pacing_enabled;
    let discord_rpc_enabled_exit = launch_settings.discord_rpc_enabled;
    let auto_backup_saves = launch_settings.auto_backup_saves_on_exit;
    let install_dir_for_exit = game_root_dir.to_string_lossy().to_string();
    let session_instant = Instant::now();

    tokio::spawn(async move {
        if let Some(ref job_arc) = job_handle_opt {
            let job_boost = Arc::clone(job_arc);
            let mode_handoff = cpu_mode_loop.clone();
            tokio::spawn(async move {
                // After bootstrapper handoff (~1.5s), promote any spawned game child PIDs too.
                tokio::time::sleep(Duration::from_millis(1500)).await;
                let tree_pids = job_boost.active_pids();
                if !tree_pids.is_empty() && should_boost_tree {
                    duke_core::win32_ffi::apply_cpu_priority_mode(&tree_pids, &mode_handoff);
                }
            });
        }

        let exit_code_opt = match spawned_child_opt.take() {
            Some(SpawnedChildKind::Tokio(mut child)) => {
                child.wait().await.ok().and_then(|st| st.code())
            }
            Some(SpawnedChildKind::JailedProcHandle(proc_handle)) => {
                tokio::task::spawn_blocking(move || duke_jail::wait_process_exit_blocking(proc_handle))
                    .await
                    .ok()
                    .flatten()
            }
            None => None,
        };

        // If the launched process acted as a bootstrapper and spawned child game processes,
        // keep tracking playtime until every descendant process in the Job Object exits.
        if let Some(ref job_arc) = job_handle_opt {
            loop {
                let remaining_pids = job_arc.active_pids();
                if remaining_pids.is_empty() {
                    break;
                }
                if should_boost_tree {
                    duke_core::win32_ffi::apply_cpu_priority_mode(&remaining_pids, &cpu_mode_loop);
                }
                tokio::time::sleep(Duration::from_millis(450)).await;
            }
        }

        // Fast crash auto-diagnosis (< 5s exit with known Windows NTSTATUS failure codes + Fix Finder telemetry)
        let raw_elapsed = session_instant.elapsed();
        if raw_elapsed < Duration::from_secs(5) {
            if let Some(code) = exit_code_opt {
                if code != 0 {
                    let diag_gid = gid_after.as_deref().unwrap_or(&session_key_after);
                    let app_id_opt = (item_app_id > 0).then_some(item_app_id);
                    let diag = fix_finder::diagnose_crash_code(
                        diag_gid,
                        &game_title_after,
                        &path_after,
                        app_id_opt,
                        code,
                        raw_elapsed.as_millis() as u64,
                    );
                    let _ = app_after.emit("game-crash-detected", &diag);
                }
                if let Some((nt_hex, nt_desc)) = self_heal::is_crash_ntstatus(code) {
                    if let Some(ref heal_gid) = gid_after {
                        if let Ok(fix_report) =
                            auto_fix_game(app_after.clone(), heal_gid.clone()).await
                        {
                            let _ = app_after.emit(
                                "game-crash-healed",
                                serde_json::json!({
                                    "gameId": heal_gid,
                                    "title": game_title_after,
                                    "exePath": path_after,
                                    "exitCodeHex": nt_hex,
                                    "diagnosis": nt_desc,
                                    "summary": fix_report.summary,
                                    "healedDlls": fix_report.healed_dlls,
                                    "unblockedFiles": fix_report.unblocked_files
                                }),
                            );
                        }
                    }
                }
            }
        }

        if let Ok(mut guard) = get_active_sessions().lock() {
            guard.remove(&session_key_after);
        }
        if let Ok(mut jguard) = get_active_jobs_map().lock() {
            jguard.remove(&session_key_after);
        }
        quick_resume::unregister_slot(&session_key_after);
        let _ = app_after.emit("quick-resume-updated", quick_resume::build_summary_snapshot());

        duke_upscale::stop_game_upscale_runtime(&session_key_after);
        let scorecard_opt = etw_xray::stop_and_persist_scorecard(
            &app_after,
            &session_key_after,
            &game_title_after,
            raw_elapsed.as_secs_f64(),
        );
        {
            let verified_path = PathBuf::from(&path_after);
            let verified_boot_ok =
                raw_elapsed >= Duration::from_secs(5) || exit_code_opt.unwrap_or(0) == 0;
            let verified_crash_code = (!verified_boot_ok)
                .then(|| exit_code_opt.map(|c| format!("0x{:08X}", c as u32)))
                .flatten();
            let verified_fps = scorecard_opt.as_ref().map(|c| c.avg_fps);
            let emu_core_opt = (!resolved_emu_id.trim().is_empty()).then_some(resolved_emu_id.clone());
            tokio::task::spawn_blocking(move || {
                if let Ok(hash) = duke_verified::hash_binary_blake3_streaming(&verified_path) {
                    let _ = duke_verified::record_verified_outcome_in_matrix(
                        &hash,
                        verified_boot_ok,
                        verified_crash_code.as_deref(),
                        verified_fps,
                        None,
                        None,
                        emu_core_opt.as_deref(),
                    );
                }
            });
        }

        if frame_pacing_enabled_exit {
            let fp = duke_core::win32_ffi::end_frame_pacing_runtime();
            let _ = app_after.emit("frame-pacing-state", &fp);
        }

        if shader_swarm_enabled_exit {
            let s_gid = gid_after.clone().unwrap_or_else(|| session_key_after.clone());
            let s_title = game_title_after.clone();
            let s_exe = PathBuf::from(&path_after);
            let s_dir = PathBuf::from(&install_dir_for_exit);
            let push_peers = swarm_enabled_exit;
            tokio::spawn(async move {
                if let Ok(Ok(meta)) = tokio::task::spawn_blocking(move || {
                    shader_swarm::capture_game_exit_shaders(
                        &s_gid,
                        &s_title,
                        &s_exe,
                        &s_dir,
                        session_launch_epoch,
                    )
                })
                .await
                {
                    if push_peers && meta.file_count > 0 {
                        let _ = swarm::push_shader_meta_to_peers(&meta).await;
                    }
                }
            });
        }

        let elapsed_secs = session_instant.elapsed().as_secs().max(1);
        let end_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Restore NORMAL_PRIORITY_CLASS (0x00000020) when game process tree exits
        restore_normal_priority_mode();

        let shm_exit_tag = if !game_title_after.trim().is_empty() {
            game_title_after.trim().to_string()
        } else {
            exe_name_after.trim().to_string()
        };
        publish_pipe_event(DukeEvent::GameExited {
            title: game_title_after.clone(),
            exe_name: exe_name_after,
            pid: game_pid,
            app_id: item_app_id,
            platform_id: item_platform_id,
            exe_hash: launch_exe_hash,
        });
        publish_shm_frame_direct(duke_core::shm_bus::ShmEventFrame::game_exited(
            ShmSourceApp::DukeLauncher,
            game_pid,
            item_app_id,
            item_platform_id,
            launch_exe_hash,
            &shm_exit_tag,
        ));
        let gid_key = gid_after.as_deref().unwrap_or(&session_key_after);
        if let Some(ref gid) = gid_after {
            achievements::stop_achievement_watcher(gid);
            time_machine::stop_save_watcher(gid);
        }
        save_watcher::stop_save_watcher_for_game(gid_key);
        let remaining_title = get_active_sessions()
            .lock()
            .ok()
            .and_then(|guard| guard.values().next().map(|s| s.title.clone()));
        match remaining_title {
            None => discord_ipc::clear_discord_presence(),
            Some(next_title) if discord_rpc_enabled_exit => {
                discord_ipc::set_discord_presence_game(&next_title, "PC Windows", None, None);
            }
            Some(_) => {}
        }
        if item_platform_id > 0 {
            let gid_snap = gid_after.clone().unwrap_or_else(|| session_key_after.clone());
            let emu_snap = resolved_emu_id.clone();
            let lbl_snap = format!("Auto-save snapshot: {}", game_title_after);
            let _ = tokio::task::spawn_blocking(move || {
                emulators::snapshot_console_saves_on_exit(
                    &gid_snap,
                    item_platform_id,
                    &emu_snap,
                    Some(&lbl_snap),
                );
            })
            .await;
        }
        if let Some(ref gid) = gid_after {
            if swarm_auto_sync {
                swarm::spawn_game_stopped_mesh_sync(gid.clone());
            }
        }
        if auto_backup_saves {
            let b_gid = gid_after.clone();
            let b_title = Some(game_title_after.clone());
            let b_dir = Some(install_dir_for_exit.clone());
            let b_exe = Some(path_after.clone());
            tokio::spawn(async move {
                let _ = ludusavi::backup_game_saves_core(b_gid, b_title, b_dir, b_exe).await;
            });
        }

        let app_disk = app_after.clone();
        let _ = tokio::task::spawn_blocking(move || {
            if let Ok(mut library) = load_library_from_disk(&app_disk) {
                if let Some(item) = library.iter_mut().find(|g| {
                    gid_after.as_ref().map(|id| g.id == *id).unwrap_or(false)
                        || g.exe_path.eq_ignore_ascii_case(&path_after)
                }) {
                    if item.imported_playtime_secs == 0
                        && item.duke_playtime_secs == 0
                        && item.playtime_seconds > 0
                    {
                        item.duke_playtime_secs = item.playtime_seconds;
                    }
                    item.duke_playtime_secs = item.duke_playtime_secs.saturating_add(elapsed_secs);
                    item.playtime_seconds = item
                        .imported_playtime_secs
                        .saturating_add(item.duke_playtime_secs);
                    item.last_played = Some(end_ts);
                    item.last_played_ts = Some(end_ts);
                    let wal_ok = resolve_data_root_dir(&app_disk)
                        .and_then(|d| {
                            lib_store::append_playtime_wal(
                                &d,
                                &item.id,
                                item.playtime_seconds,
                                end_ts,
                            )
                        })
                        .is_ok();
                    if !wal_ok {
                        let _ = save_library_to_disk(&app_disk, &library);
                    }
                    let _ = app_disk.emit("library-updated", &library);
                }
            }
        })
        .await;

        playtime_sync::spawn_background_playtime_sync(app_after.clone());
        focus_main_window(&app_after);
        trim_process_working_set();
        let _ = app_after.emit(
            "game-running-state",
            serde_json::json!({ "running": false, "gameId": session_key_after, "isSuspended": false, "gameBoost": false }),
        );
        let _ = app_after.emit(
            "game-session-state",
            serde_json::json!({ "running": false, "gameId": session_key_after, "isSuspended": false, "gameBoost": false }),
        );
        let _ = app_after.emit(
            "memory-telemetry",
            serde_json::json!({
                "workingSetMb": get_process_working_set_mb(),
                "dohShield": "ACTIVE",
                "status": "IDLE TRIMMED"
            }),
        );
    });

    Ok(())
}

pub(crate) fn spawn_hotkey_watcher_if_needed(app: AppHandle) {
    if HOTKEY_WATCHER_ACTIVE
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    tauri::async_runtime::spawn(async move {
        let mut was_pressed = false;
        let mut pause_was_pressed = false;
        let mut ticks_elapsed: u64 = 0;
        let mut auto_frozen_sessions: HashSet<String> = HashSet::new();
        loop {
            tokio::time::sleep(Duration::from_millis(120)).await;
            ticks_elapsed = ticks_elapsed.saturating_add(1);
            let sessions_snapshot: Vec<ActiveGameSession> = {
                get_active_sessions()
                    .lock()
                    .map(|g| g.values().cloned().collect())
                    .unwrap_or_default()
            };
            if sessions_snapshot.is_empty() {
                break;
            }

            let hk_str = get_active_hotkey_cell()
                .lock()
                .map(|s| s.clone())
                .unwrap_or_else(|_| "Ctrl+Alt+P".to_string());

            let configured_pressed = duke_core::win32_ffi::parse_hotkey_spec(&hk_str)
                .map(|spec| duke_core::win32_ffi::is_hotkey_pressed(&spec))
                .unwrap_or(false);
            let pause_spec = duke_core::win32_ffi::HotkeySpec {
                ctrl: false,
                alt: false,
                shift: false,
                key_vk: 0x13,
            };
            let pause_pressed = duke_core::win32_ffi::is_hotkey_pressed(&pause_spec);

            let edge_configured = configured_pressed && !was_pressed;
            let edge_pause = pause_pressed && !pause_was_pressed && hk_str.eq_ignore_ascii_case("Ctrl+Alt+P");
            was_pressed = configured_pressed;
            pause_was_pressed = pause_pressed;

            if edge_configured || edge_pause {
                if let Some(first) = sessions_snapshot.first() {
                    auto_frozen_sessions.remove(&first.game_id);
                    let _ = suspend_or_resume_game(app.clone(), Some(first.game_id.clone()), None).await;
                }
            }

            // Poll minimized window state ~every 1.2s after a 15s startup grace period
            if AUTO_FREEZE_MINIMIZED.load(Ordering::Relaxed) && ticks_elapsed > 125 && ticks_elapsed.is_multiple_of(10) {
                for sess in &sessions_snapshot {
                    let mut pids = get_active_jobs_map()
                        .lock()
                        .ok()
                        .and_then(|map| map.get(&sess.game_id).map(|job| job.active_pids()))
                        .unwrap_or_default();
                    if pids.is_empty() && sess.pid > 0 {
                        pids.push(sess.pid);
                    }
                    let minimized = duke_core::win32_ffi::is_process_tree_minimized(&pids);
                    if minimized && !sess.is_suspended && !auto_frozen_sessions.contains(&sess.game_id) {
                        if suspend_or_resume_game(app.clone(), Some(sess.game_id.clone()), Some("suspend".to_string())).await.is_ok() {
                            auto_frozen_sessions.insert(sess.game_id.clone());
                        }
                    } else if !minimized && sess.is_suspended && auto_frozen_sessions.remove(&sess.game_id) {
                        let _ = suspend_or_resume_game(app.clone(), Some(sess.game_id.clone()), Some("resume".to_string())).await;
                    }
                }
            } else if !AUTO_FREEZE_MINIMIZED.load(Ordering::Relaxed) && !auto_frozen_sessions.is_empty() {
                auto_frozen_sessions.clear();
            }
        }
        HOTKEY_WATCHER_ACTIVE.store(false, Ordering::SeqCst);
    });
}

pub(crate) async fn suspend_or_resume_game_core(
    app: AppHandle,
    game_id: Option<String>,
    action: Option<String>,
) -> Result<SuspendStateResult, String> {
    suspend_or_resume_game(app, game_id, action).await
}

#[tauri::command]
pub(crate) async fn suspend_or_resume_game(
    app: AppHandle,
    game_id: Option<String>,
    action: Option<String>,
) -> Result<SuspendStateResult, String> {
    let target_gid = game_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let act = action.unwrap_or_else(|| "toggle".to_string()).to_lowercase();

    let (session_key, title, pid, currently_suspended) = {
        let guard = get_active_sessions().lock().unwrap_or_else(|e| e.into_inner());
        let found = if let Some(ref gid) = target_gid {
            guard.get(gid).cloned().or_else(|| {
                guard
                    .values()
                    .find(|s| s.game_id == *gid || s.title.eq_ignore_ascii_case(gid))
                    .cloned()
            })
        } else {
            guard.values().next().cloned()
        };
        let Some(sess) = found else {
            return Err("No active running game session found to suspend or resume.".to_string());
        };
        (sess.game_id, sess.title, sess.pid, sess.is_suspended)
    };

    let want_suspend = match act.as_str() {
        "suspend" => true,
        "resume" => false,
        _ => !currently_suspended,
    };

    let target_pids: Vec<u32> = {
        let mut pids = get_active_jobs_map()
            .lock()
            .ok()
            .and_then(|map| map.get(&session_key).map(|job| job.active_pids()))
            .unwrap_or_default();
        if pids.is_empty() && pid > 0 {
            pids.push(pid);
        }
        pids
    };

    let freed_ram_mb = if want_suspend {
        let mb = duke_core::win32_ffi::suspend_process_tree(&target_pids)?;
        duke_core::win32_ffi::trim_current_working_set();
        mb
    } else {
        duke_core::win32_ffi::resume_process_tree(&target_pids)?;
        duke_core::win32_ffi::boost_process_tree(&target_pids);
        0.0
    };

    {
        let mut guard = get_active_sessions().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(sess) = guard.get_mut(&session_key) {
            sess.is_suspended = want_suspend;
        }
    }
    quick_resume::sync_slot_suspend_state(&session_key, want_suspend, None);
    let _ = app.emit("quick-resume-updated", quick_resume::build_summary_snapshot());

    let result = SuspendStateResult {
        game_id: session_key.clone(),
        title: title.clone(),
        pid,
        is_suspended: want_suspend,
        freed_ram_mb,
    };

    let _ = app.emit("game-suspend-state", &result);
    let _ = app.emit(
        "game-running-state",
        serde_json::json!({
            "running": true,
            "gameId": session_key,
            "pid": pid,
            "isSuspended": want_suspend,
            "gameBoost": !want_suspend
        }),
    );
    let _ = app.emit(
        "memory-telemetry",
        serde_json::json!({
            "workingSetMb": get_process_working_set_mb(),
            "dohShield": "ACTIVE",
            "status": if want_suspend { "GAME SUSPENDED" } else { "GAME BOOST" }
        }),
    );

    Ok(result)
}

#[tauri::command]
pub(crate) async fn get_active_game_sessions() -> Vec<ActiveGameSession> {
    get_active_sessions()
        .lock()
        .map(|g| g.values().cloned().collect())
        .unwrap_or_default()
}

#[tauri::command]
pub(crate) async fn poll_gamepad_state() -> Result<couch_mode::UniversalPadState, String> {
    run_blocking_timeout(5, "poll_gamepad_state", || {
        Ok(couch_mode::poll_universal_controller_state())
    })
    .await
}

pub(crate) async fn launch_game_detached_headless(
    exe_path: &str,
    game_id: Option<&str>,
    launch_args: Option<&str>,
) -> Result<u32, String> {
    let trimmed = exe_path.trim().trim_matches('"');
    let parsed_args = launch_args.map(parse_cli_launch_args).unwrap_or_default();

    let gid_lookup = game_id.map(str::to_string);
    let exe_lookup = trimmed.to_string();
    let (
        sandbox_enabled,
        prefetch_enabled,
        game_root_dir,
        item_platform_id,
        item_app_id,
        item_emulator_id,
        resolved_exe_from_lib,
        resolved_game_title,
        launch_settings,
    ) = tokio::task::spawn_blocking(move || {
        let global_settings = load_settings_standalone();
        let mut sb = global_settings.portable_sandbox_default;
        let mut pf = global_settings.performance_boost_enabled
            && global_settings.nvme_prefetch_enabled
            && global_settings.nvme_cache_budget_pct > 0;
        let mut g_root = resolve_game_root_dir(Path::new(&exe_lookup));
        let mut plat_id = 0u16;
        let mut app_id = 0u32;
        let mut emu_id: Option<String> = None;
        let mut lib_exe: Option<String> = None;
        let mut lib_title = String::new();
        if let Ok(library) = load_library_standalone() {
            if let Some(item) = library.iter().find(|g| {
                gid_lookup.as_ref().map(|id| g.id == *id).unwrap_or(false)
                    || (!exe_lookup.is_empty() && g.exe_path.eq_ignore_ascii_case(&exe_lookup))
            }) {
                plat_id = item.platform_id;
                app_id = item
                    .app_id
                    .or_else(|| item.steam_app_id.and_then(|v| u32::try_from(v).ok()))
                    .unwrap_or(0);
                if !item.title.trim().is_empty() {
                    lib_title = item.title.trim().to_string();
                }
                if !item.emulator_id.trim().is_empty() {
                    emu_id = Some(item.emulator_id.trim().to_string());
                }
                if exe_lookup.is_empty() && !item.exe_path.trim().is_empty() {
                    lib_exe = Some(item.exe_path.trim().to_string());
                }
                sb = item.sandbox_enabled || global_settings.portable_sandbox_default;
                let item_pf = item
                    .nvme_prefetch_enabled
                    .unwrap_or(global_settings.nvme_prefetch_enabled);
                pf = global_settings.performance_boost_enabled
                    && item_pf
                    && global_settings.nvme_cache_budget_pct > 0;
                if !item.install_dir.trim().is_empty() {
                    let cand = PathBuf::from(item.install_dir.trim());
                    if cand.is_dir() {
                        g_root = cand;
                    }
                }
            }
        }
        (
            sb,
            pf,
            g_root,
            plat_id,
            app_id,
            emu_id,
            lib_exe,
            lib_title,
            global_settings,
        )
    })
    .await
    .unwrap_or_else(|_| {
        let fallback = load_settings_standalone();
        (
            fallback.portable_sandbox_default,
            fallback.performance_boost_enabled
                && fallback.nvme_prefetch_enabled
                && fallback.nvme_cache_budget_pct > 0,
            PathBuf::new(),
            0,
            0,
            None,
            None,
            String::new(),
            fallback,
        )
    });

    let effective_target = if trimmed.is_empty() {
        resolved_exe_from_lib.unwrap_or_default()
    } else {
        trimmed.to_string()
    };
    let exe_file = PathBuf::from(&effective_target);
    if !exe_file.exists() || !exe_file.is_file() {
        return Err(format!("Game executable not found at '{}'", effective_target));
    }
    unblock_ntfs_zone_identifier(&exe_file);

    let (spawn_exe, working_dir, emu_args) = if item_platform_id > 0 {
        let resolved = emulators::resolve_emulator_launch(
            item_platform_id,
            item_emulator_id.as_deref(),
            &exe_file,
        )?;
        unblock_ntfs_zone_identifier(&resolved.exe_path);
        (resolved.exe_path, resolved.working_dir, resolved.args)
    } else {
        let wd = exe_file
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| "Unable to determine executable working directory.".to_string())?;
        (exe_file.clone(), wd, Vec::new())
    };

    if prefetch_enabled && item_platform_id > 0 {
        let rom_len = fs::metadata(&exe_file).map(|m| m.len()).unwrap_or(0);
        if rom_len > 32 * 1024 * 1024 {
            let warm_rom = exe_file.clone();
            let budget_pct = launch_settings.nvme_cache_budget_pct.clamp(1, 40);
            let _ = tokio::task::spawn_blocking(move || {
                duke_core::nvme_prefetch::warm_single_file_with_budget_pct(&warm_rom, budget_pct)
            })
            .await;
        }
    }

    let mut combined_args = if item_platform_id > 0 {
        emu_args
    } else {
        launch_settings
            .global_launch_args
            .as_deref()
            .map(parse_cli_launch_args)
            .unwrap_or_default()
    };
    combined_args.extend(parsed_args);

    let mut cmd = Command::new(&spawn_exe);
    cmd.current_dir(&working_dir)
        .args(&combined_args)
        .env("STEAM_FRAME_FORCE_CLOSE", "1");
    if launch_settings.block_game_telemetry {
        cmd.env("DO_NOT_TRACK", "1")
            .env("SENTRY_DSN", "")
            .env("UNITY_DISABLE_ANALYTICS", "1")
            .env("UE_DISABLE_CRASH_REPORTER", "1");
    }

    if sandbox_enabled {
        if let Ok(env_overrides) = pe_inspect::prepare_game_sandbox_env(&game_root_dir) {
            for (k, v) in env_overrides {
                cmd.env(k, v);
            }
        }
    }

    if let Some(stage_gid) = game_id {
        let _ = vhdx_stage::ensure_game_hot_staged_before_launch(stage_gid, &game_root_dir);
    }
    latency_lab::apply_saved_profile_if_present(&effective_target);
    for (k, v) in latency_lab::shader_cache_launch_env() {
        cmd.env(k, v);
    }

    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed spawning '{}': {}", effective_target, e))?;
    let pid = child.id();
    if item_platform_id > 0
        || launch_settings.performance_boost_enabled
        || launch_settings.cpu_priority_mode != "auto"
    {
        duke_core::win32_ffi::apply_cpu_priority_mode(&[pid], &launch_settings.cpu_priority_mode);
    }
    let exe_tag = spawn_exe
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let standalone_exe_hash =
        duke_core::shm_bus::fnv1a_hash64(effective_target.to_ascii_lowercase().as_bytes());
    let standalone_label = if !resolved_game_title.is_empty() {
        resolved_game_title.as_str()
    } else {
        exe_tag
    };
    publish_pipe_event(DukeEvent::GameLaunched {
        title: standalone_label.to_string(),
        exe_name: exe_tag.to_string(),
        pid,
        app_id: item_app_id,
        platform_id: item_platform_id,
        exe_hash: standalone_exe_hash,
    });
    publish_shm_frame_direct(duke_core::shm_bus::ShmEventFrame::game_launched(
        ShmSourceApp::DukeLauncher,
        pid,
        item_app_id,
        item_platform_id,
        sandbox_enabled,
        standalone_exe_hash,
        standalone_label,
    ));

    if prefetch_enabled && item_platform_id == 0 && game_root_dir.is_dir() {
        let warm_dir = game_root_dir.clone();
        let budget_pct = launch_settings.nvme_cache_budget_pct.clamp(1, 40);
        tokio::task::spawn_blocking(move || {
            let _ = duke_core::nvme_prefetch::warm_game_assets_with_budget_pct(&warm_dir, budget_pct);
        });
    }

    let start_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let trimmed_owned = effective_target;
    let gid_owned = game_id.map(str::to_string);
    let _ = tokio::task::spawn_blocking(move || {
        if let Ok(mut library) = load_library_standalone() {
            if let Some(item) = library.iter_mut().find(|g| {
                gid_owned.as_ref().map(|id| g.id == *id).unwrap_or(false)
                    || g.exe_path.eq_ignore_ascii_case(&trimmed_owned)
            }) {
                item.last_played = Some(start_ts);
                item.last_played_ts = Some(start_ts);
                let _ = save_library_standalone(&library);
            }
        }
    })
    .await;

    Ok(pid)
}

#[tauri::command]
pub(crate) async fn launch_local_game(
    app: AppHandle,
    window: WebviewWindow,
    exe_path: String,
    game_id: Option<String>,
    launch_args: Option<String>,
) -> Result<(), String> {
    launch_game_internal(app, Some(window), exe_path, game_id, launch_args).await
}

#[tauri::command]
pub(crate) async fn launch_game(
    app: AppHandle,
    window: WebviewWindow,
    exe_path: String,
    game_id: Option<String>,
    launch_args: Option<String>,
) -> Result<(), String> {
    launch_game_internal(app, Some(window), exe_path, game_id, launch_args).await
}

pub(crate) async fn launch_game_headless(
    app: AppHandle,
    exe_path: String,
    game_id: Option<String>,
    launch_args: Option<String>,
) -> Result<(), String> {
    let win = app.get_webview_window("main");
    launch_game_internal(app, win, exe_path, game_id, launch_args).await
}

#[tauri::command]
pub(crate) async fn trim_working_set(app: AppHandle) -> Result<Value, String> {
    let payload = run_blocking_timeout(5, "trim_working_set", || {
        trim_process_working_set();
        let ram_mb = get_process_working_set_mb();
        publish_shm_event(
            ShmEventType::RamPurge,
            std::process::id(),
            ram_mb.to_bits(),
            0,
        );
        let doh_hosts = duke_core::net_doh::cached_doh_host_count();
        Ok(serde_json::json!({
            "workingSetMb": ram_mb,
            "dohShield": "ACTIVE",
            "dohCachedHosts": doh_hosts,
            "portableMode": resolve_portable_data_dir().is_some(),
            "status": "IDLE TRIMMED"
        }))
    })
    .await?;
    let _ = app.emit("memory-telemetry", &payload);
    Ok(payload)
}

#[tauri::command]
pub(crate) async fn get_system_ram_mb() -> Result<f64, String> {
    run_blocking_timeout(5, "get_system_ram_mb", || Ok(get_process_working_set_mb())).await
}

#[tauri::command]
pub(crate) async fn check_for_app_updates(
    force: Option<bool>,
) -> Result<updater::UpdateCheckReport, String> {
    if !force.unwrap_or(true) {
        if let Some(cached) = updater::get_cached_update_report() {
            return Ok(cached);
        }
    }
    updater::check_for_app_updates_internal().await
}

#[tauri::command]
pub(crate) async fn get_cached_app_update() -> Result<Option<updater::UpdateCheckReport>, String> {
    Ok(updater::get_cached_update_report())
}

#[tauri::command]
pub(crate) async fn download_and_apply_app_update(
    app: AppHandle,
    version: Option<String>,
) -> Result<updater::UpdateApplyResult, String> {
    updater::download_and_apply_app_update_internal(&app, version).await
}

#[tauri::command]
pub(crate) async fn get_system_telemetry() -> Result<Value, String> {
    run_blocking_timeout(5, "get_system_telemetry", || {
        let ram_mb = get_process_working_set_mb();
        let doh_hosts = duke_core::net_doh::cached_doh_host_count();
        Ok(serde_json::json!({
            "workingSetMb": ram_mb,
            "dohShield": if duke_core::net_doh::is_doh_enabled() { "ACTIVE" } else { "SYSTEM DNS" },
            "dohCachedHosts": doh_hosts,
            "portableMode": resolve_portable_data_dir().is_some(),
            "status": "IDLE TRIMMED"
        }))
    })
    .await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskSpaceInfo {
    pub path: String,
    pub root_drive: String,
    pub free_bytes: u64,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_pct: f64,
    pub used_pct: f64,
    pub free_formatted: String,
    pub total_formatted: String,
}

#[cfg(target_os = "windows")]
pub(crate) fn query_disk_free_space_sync(target_dir: &Path) -> Result<(u64, u64), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let mut check_dir = target_dir.to_path_buf();
    while !check_dir.exists() {
        let Some(parent) = check_dir.parent() else {
            break;
        };
        check_dir = parent.to_path_buf();
    }
    let query_path = if check_dir.exists() {
        check_dir
    } else {
        PathBuf::from("C:\\")
    };
    let wide: Vec<u16> = query_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut free_avail = 0u64;
    let mut total_bytes = 0u64;
    unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR(wide.as_ptr()),
            Some(&mut free_avail),
            Some(&mut total_bytes),
            None,
        )
        .map_err(|e| format!("GetDiskFreeSpaceExW failed: {}", e))?;
    }
    Ok((free_avail, total_bytes))
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn query_disk_free_space_sync(_target_dir: &Path) -> Result<(u64, u64), String> {
    Ok((0, 0))
}

/// Returns live drive space telemetry for the configured game save folder.
#[tauri::command]
pub(crate) async fn get_disk_free_space(
    app: AppHandle,
    path: Option<String>,
) -> Result<DiskSpaceInfo, String> {
    let target = match path.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        Some(custom) => PathBuf::from(custom),
        None => resolve_default_downloads_dir(&app),
    };
    let path_display = target.to_string_lossy().to_string();
    let root_drive = target
        .components()
        .next()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .unwrap_or_else(|| "Drive".to_string());

    let target_clone = target.clone();
    let (free_bytes, total_bytes) = tokio::task::spawn_blocking(move || query_disk_free_space_sync(&target_clone))
        .await
        .map_err(|e| format!("Disk space worker failed: {}", e))??;

    let used_bytes = total_bytes.saturating_sub(free_bytes);
    let free_pct = if total_bytes > 0 {
        ((free_bytes as f64 / total_bytes as f64) * 1000.0).round() / 10.0
    } else {
        0.0
    };
    let used_pct = (100.0 - free_pct).clamp(0.0, 100.0);

    Ok(DiskSpaceInfo {
        path: path_display,
        root_drive,
        free_bytes,
        total_bytes,
        used_bytes,
        free_pct,
        used_pct,
        free_formatted: format_bytes(free_bytes),
        total_formatted: format_bytes(total_bytes),
    })
}

/// Registers `duke://` browser download protocol handler in `HKCU\Software\Classes\duke`.
#[tauri::command]
pub(crate) async fn register_duke_protocol() -> Result<String, String> {
    run_blocking_timeout(5, "register_duke_protocol", || {
        win32_clipboard_protocol::register_duke_uri_protocol();
        Ok("Registered duke:// one-click browser download links with Windows.".to_string())
    })
    .await
}

/// Runs the Duke Suite hardware subsystem microbenchmark suite inline for Advanced Engine settings.
#[tauri::command]
pub(crate) async fn run_subsystem_benchmark() -> Result<Value, String> {
    tokio::task::spawn_blocking(|| {
        let report = bench::run_benchmarks()?;
        let table = bench::render_benchmark_table(&report);
        Ok(serde_json::json!({
            "report": report,
            "table": table,
        }))
    })
    .await
    .map_err(|e| format!("Benchmark worker failed: {}", e))?
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateFeedMetadata {
    pub version: String,
    pub notes: String,
    pub manifest_url: Option<String>,
    pub delta_url: Option<String>,
    pub archive_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct GameUpdateCheckResult {
    pub game_id: String,
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub notes: String,
    pub has_delta_patch: bool,
    pub download_url: String,
    pub games: Vec<GameEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct GameUpdateApplyReport {
    pub game_id: String,
    pub previous_version: String,
    pub new_version: String,
    pub method: String,
    pub snapshot_id: String,
    pub message: String,
    pub games: Vec<GameEntry>,
}

pub(crate) fn parse_version_parts(raw: &str) -> Vec<u64> {
    let trimmed = raw
        .trim()
        .trim_start_matches(['v', 'V'])
        .split(['-', '+'])
        .next()
        .unwrap_or("");
    let mut parts = Vec::new();
    for seg in trimmed.split('.') {
        if let Ok(n) = seg.trim().parse::<u64>() {
            parts.push(n);
        } else {
            return Vec::new();
        }
    }
    parts
}

pub fn is_newer_version(current: &str, candidate: &str) -> bool {
    let cur = current.trim();
    let cand = candidate.trim();
    if cand.is_empty() {
        return false;
    }
    if cur.is_empty() {
        return true;
    }
    if cur.trim_start_matches(['v', 'V']).eq_ignore_ascii_case(cand.trim_start_matches(['v', 'V'])) {
        return false;
    }
    let a = parse_version_parts(cur);
    let b = parse_version_parts(cand);
    if !a.is_empty() && !b.is_empty() {
        let len = a.len().max(b.len());
        for idx in 0..len {
            let av = *a.get(idx).unwrap_or(&0);
            let bv = *b.get(idx).unwrap_or(&0);
            if bv != av {
                return bv > av;
            }
        }
        return false;
    }
    cand != cur
}

pub fn parse_update_feed_metadata(val: &Value) -> Result<UpdateFeedMetadata, String> {
    let version = val
        .get("version")
        .or_else(|| val.get("versionTag"))
        .or_else(|| val.get("latestVersion"))
        .or_else(|| val.get("tag_name"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Update feed JSON missing 'version' or 'tag_name' field".to_string())?
        .to_string();

    let notes = val
        .get("notes")
        .or_else(|| val.get("changelog"))
        .or_else(|| val.get("body"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();

    let pick_str = |keys: &[&str]| -> Option<String> {
        keys.iter().find_map(|k| {
            val.get(*k)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
                .map(ToString::to_string)
        })
    };

    let mut manifest_url = pick_str(&["manifestUrl", "manifest_url", "dkmanifestUrl"]);
    let mut delta_url = pick_str(&["deltaUrl", "delta_url", "dkdeltaUrl", "patchUrl"]);
    let mut archive_url = pick_str(&["archiveUrl", "archive_url", "downloadUrl", "download_url", "url"]);

    if let Some(assets) = val.get("assets").and_then(|v| v.as_array()) {
        for asset in assets {
            let url = asset
                .get("browser_download_url")
                .or_else(|| asset.get("url"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .unwrap_or_default();
            if !url.starts_with("http") {
                continue;
            }
            let lower = url.to_ascii_lowercase();
            if lower.ends_with(".dkmanifest") && manifest_url.is_none() {
                manifest_url = Some(url.to_string());
            } else if lower.ends_with(".dkdelta") && delta_url.is_none() {
                delta_url = Some(url.to_string());
            } else if (lower.ends_with(".tar.zst") || lower.ends_with(".tzst") || lower.ends_with(".zip"))
                && archive_url.is_none()
            {
                archive_url = Some(url.to_string());
            }
        }
    }

    Ok(UpdateFeedMetadata {
        version,
        notes,
        manifest_url,
        delta_url,
        archive_url,
    })
}

pub(crate) async fn download_url_bytes_via_doh(url: &str, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed creating staging directory {}: {}", parent.display(), e))?;
    }
    let client = duke_core::net_doh::build_doh_client_with_timeouts(
        url,
        Duration::from_secs(12),
        Duration::from_secs(120),
        true,
    )
    .await;
    let resp = client
        .get(url)
        .header(USER_AGENT, concat!("Duke-Launcher/", env!("CARGO_PKG_VERSION")))
        .send()
        .await
        .map_err(|e| format!("Download request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Server returned HTTP {}", resp.status().as_u16()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Failed reading update payload: {e}"))?;
    fs::write(dest, &bytes)
        .map_err(|e| format!("Failed writing staged update {}: {}", dest.display(), e))?;
    Ok(())
}

pub(crate) struct DukeProfilePreserveGuard {
    game_dir: PathBuf,
    backup_dir: PathBuf,
    moved: bool,
}

impl DukeProfilePreserveGuard {
    fn protect(game_dir: &Path) -> Result<Self, String> {
        let profile = game_dir.join(".duke-profile");
        let backup = game_dir.join(".duke-profile-preserve-tmp");
        let mut moved = false;
        if profile.is_dir() {
            if backup.exists() {
                let _ = fs::remove_dir_all(&backup);
            }
            fs::rename(&profile, &backup)
                .map_err(|e| format!("Failed protecting .duke-profile saves before update: {e}"))?;
            moved = true;
        }
        Ok(Self {
            game_dir: game_dir.to_path_buf(),
            backup_dir: backup,
            moved,
        })
    }
}

impl Drop for DukeProfilePreserveGuard {
    fn drop(&mut self) {
        if self.moved && self.backup_dir.is_dir() {
            let profile = self.game_dir.join(".duke-profile");
            if profile.exists() {
                let _ = fs::remove_dir_all(&profile);
            }
            let _ = fs::rename(&self.backup_dir, &profile);
        }
    }
}

#[tauri::command]
pub(crate) async fn set_game_update_config(
    app: AppHandle,
    game_id: String,
    version_tag: String,
    update_feed_url: String,
) -> Result<Vec<GameEntry>, String> {
    let mut library = load_library_from_disk(&app)?;
    let entry = library
        .iter_mut()
        .find(|g| g.id == game_id)
        .ok_or_else(|| "Game not found in library.".to_string())?;
    entry.version_tag = version_tag.trim().to_string();
    entry.update_feed_url = update_feed_url.trim().to_string();
    if entry.update_feed_url.is_empty() {
        entry.latest_version_tag.clear();
        entry.update_available = false;
    } else if !entry.latest_version_tag.is_empty() {
        entry.update_available = is_newer_version(&entry.version_tag, &entry.latest_version_tag);
    }
    save_library_to_disk(&app, &library)?;
    let _ = app.emit("library-updated", &library);
    Ok(library)
}

#[tauri::command]
pub(crate) async fn check_game_updates(
    app: AppHandle,
    game_id: Option<String>,
) -> Result<GameUpdateCheckResult, String> {
    let mut library = load_library_from_disk(&app)?;
    let targets: Vec<(usize, String, String, String)> = library
        .iter()
        .enumerate()
        .filter(|(_, g)| {
            !g.update_feed_url.trim().is_empty()
                && game_id
                    .as_deref()
                    .map(|id| g.id == id)
                    .unwrap_or(true)
        })
        .map(|(i, g)| {
            (
                i,
                g.id.clone(),
                g.version_tag.clone(),
                g.update_feed_url.clone(),
            )
        })
        .collect();

    if targets.is_empty() {
        return Err("No update feed URL configured for target game.".to_string());
    }

    let mut primary_result = GameUpdateCheckResult::default();
    let mut any_changed = false;

    for (idx, gid, cur_ver, feed_url) in targets {
        let json_val = fetch_doh_json(&feed_url).await?;
        let meta = parse_update_feed_metadata(&json_val)?;
        let avail = is_newer_version(&cur_ver, &meta.version);

        library[idx].latest_version_tag = meta.version.clone();
        library[idx].update_available = avail;
        any_changed = true;

        if game_id.as_deref() == Some(gid.as_str()) || primary_result.game_id.is_empty() {
            primary_result = GameUpdateCheckResult {
                game_id: gid,
                current_version: cur_ver,
                latest_version: meta.version,
                update_available: avail,
                notes: meta.notes,
                has_delta_patch: meta.delta_url.is_some(),
                download_url: meta
                    .delta_url
                    .or(meta.archive_url)
                    .unwrap_or_default(),
                games: Vec::new(),
            };
        }
    }

    if any_changed {
        save_library_to_disk(&app, &library)?;
        let _ = app.emit("library-updated", &library);
    }
    primary_result.games = library;
    Ok(primary_result)
}

#[tauri::command]
pub(crate) async fn apply_game_update(
    app: AppHandle,
    game_id: String,
) -> Result<GameUpdateApplyReport, String> {
    let library = load_library_from_disk(&app)?;
    let game = library
        .iter()
        .find(|g| g.id == game_id)
        .cloned()
        .ok_or_else(|| "Game not found in library.".to_string())?;
    let feed_url = game.update_feed_url.trim().to_string();
    if feed_url.is_empty() {
        return Err("Configure an Update Feed URL first.".to_string());
    }

    let game_dir = if !game.install_dir.trim().is_empty() {
        PathBuf::from(game.install_dir.trim())
    } else {
        resolve_game_root_dir(Path::new(game.exe_path.trim()))
    };
    if !game_dir.is_dir() {
        return Err(format!("Game directory missing: {}", game_dir.display()));
    }

    let feed_json = fetch_doh_json(&feed_url).await?;
    let meta = parse_update_feed_metadata(&feed_json)?;

    // Always create a zero-duplication BLAKE3 hardlink snapshot BEFORE touching game files.
    let dir_for_snap = game_dir.clone();
    let gid_for_snap = game.id.clone();
    let snap = tokio::task::spawn_blocking(move || {
        snapshots::create_snapshot_in_dir(
            &gid_for_snap,
            &dir_for_snap,
            &dir_for_snap.join(snapshots::CAS_DIR_NAME),
            Some("Auto-Backup Before Portable Update"),
        )
    })
    .await
    .map_err(|e| format!("Pre-update snapshot task failed: {e}"))??;

    let tmp_dir = game_dir.join(".duke-patch-tmp");
    let _ = fs::create_dir_all(&tmp_dir);

    let method_used: String;
    if let (Some(delta_url), Some(manifest_url)) = (meta.delta_url.clone(), meta.manifest_url.clone()) {
        let manifest_path = tmp_dir.join("target.dkmanifest");
        let delta_path = tmp_dir.join("patch.dkdelta");
        download_url_bytes_via_doh(&manifest_url, &manifest_path).await?;
        download_url_bytes_via_doh(&delta_url, &delta_path).await?;
        let dir_clone = game_dir.clone();
        let report = tokio::task::spawn_blocking(move || {
            let manifest = delta_patch::read_dkmanifest_file(&manifest_path)?;
            delta_patch::apply_delta_with_local_stitching(&dir_clone, &manifest, Some(delta_path.as_path()))
        })
        .await
        .map_err(|e| format!("Delta apply worker failed: {e}"))??;
        method_used = format!(
            "FastCDC Delta ({} files patched, {} chunks reused)",
            report.files_patched, report.chunks_reused_from_disk
        );
    } else if let Some(archive_url) = meta.archive_url.clone() {
        let lower_url = archive_url.to_ascii_lowercase();
        let dir_for_guard = game_dir.clone();
        let _save_guard = DukeProfilePreserveGuard::protect(&dir_for_guard)?;

        if lower_url.ends_with(".tar.zst") || lower_url.ends_with(".tzst") {
            let pipeline = stream_extract::InFlightZstPipeline::start(
                stream_extract::StreamArchiveFormat::TarZst,
                game_dir.clone(),
                game_dir.join("update.tar.zst"),
            );
            let client = duke_core::net_doh::build_doh_client_with_timeouts(
                &archive_url,
                Duration::from_secs(12),
                Duration::from_secs(180),
                true,
            )
            .await;
            let mut resp = client
                .get(&archive_url)
                .header(USER_AGENT, concat!("Duke-Launcher/", env!("CARGO_PKG_VERSION")))
                .send()
                .await
                .map_err(|e| format!("Streaming update download failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Server returned HTTP {}", resp.status().as_u16()));
            }
            let mut offset = 0u64;
            while let Some(chunk) = resp
                .chunk()
                .await
                .map_err(|e| format!("Stream read failed: {e}"))?
            {
                pipeline.submit_range(offset, &chunk)?;
                offset += chunk.len() as u64;
            }
            let stats = pipeline.finish()?;
            method_used = format!(
                "In-Flight .tar.zst ({} files extracted)",
                stats.files_extracted
            );
        } else {
            let ext = if lower_url.ends_with(".7z") {
                "7z"
            } else if lower_url.ends_with(".rar") {
                "rar"
            } else {
                "zip"
            };
            let pkg_path = tmp_dir.join(format!("update.{ext}"));
            download_url_bytes_via_doh(&archive_url, &pkg_path).await?;
            let pkg_clone = pkg_path.clone();
            let dest_clone = game_dir.clone();
            tokio::task::spawn_blocking(move || extract_archive_to_folder_sync(&pkg_clone, &dest_clone))
                .await
                .map_err(|e| format!("Update extraction task failed: {e}"))??;
            method_used = format!("Archive Overlay (.{ext})");
        }
    } else {
        let _ = fs::remove_dir_all(&tmp_dir);
        return Err("Update feed specifies neither a FastCDC deltaUrl nor an archiveUrl.".to_string());
    }

    let _ = fs::remove_dir_all(&tmp_dir);

    let dir_for_size = game_dir.clone();
    let new_size = tokio::task::spawn_blocking(move || calculate_dir_size_sync(&dir_for_size))
        .await
        .unwrap_or(game.size_bytes);

    let mut updated_lib = load_library_from_disk(&app)?;
    if let Some(entry) = updated_lib.iter_mut().find(|g| g.id == game.id) {
        entry.version_tag = meta.version.clone();
        entry.latest_version_tag = meta.version.clone();
        entry.update_available = false;
        entry.size_bytes = new_size;
        entry.size_formatted = format_bytes(new_size);
    }
    save_library_to_disk(&app, &updated_lib)?;
    let _ = app.emit("library-updated", &updated_lib);
    trim_process_working_set();

    Ok(GameUpdateApplyReport {
        game_id: game.id,
        previous_version: game.version_tag,
        new_version: meta.version,
        method: method_used.clone(),
        snapshot_id: snap.id.clone(),
        message: format!(
            "Updated via {method_used} (.duke-cas snapshot {} created; .duke-profile saves preserved).",
            snap.id
        ),
        games: updated_lib,
    })
}

#[tauri::command]
pub(crate) async fn toggle_overlay_window(app: AppHandle, visible: Option<bool>) -> Result<bool, String> {
    let overlay = app
        .get_webview_window("overlay")
        .ok_or_else(|| "Overlay window not registered".to_string())?;
    let currently_visible = overlay.is_visible().unwrap_or(false);
    let target_visible = visible.unwrap_or(!currently_visible);

    if target_visible {
        let _ = overlay.set_always_on_top(true);
        #[cfg(target_os = "windows")]
        {
            if let Ok(hwnd) = overlay.hwnd() {
                let _ = apply_backdrop_to_hwnd(hwnd.0 as isize, "acrylic");
            }
        }
        let _ = overlay.show();
        let _ = overlay.set_focus();
        let _ = app.emit("overlay-opened", serde_json::json!({ "visible": true }));
    } else {
        let _ = overlay.hide();
        let _ = app.emit("overlay-opened", serde_json::json!({ "visible": false }));
        trim_process_working_set();
    }
    Ok(target_visible)
}

#[tauri::command]
pub(crate) async fn hide_overlay_window(app: AppHandle) -> Result<(), String> {
    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.hide();
    }
    trim_process_working_set();
    Ok(())
}

#[tauri::command]
pub(crate) async fn trigger_replay_clip_ipc() -> Result<Value, String> {
    let active_title = get_active_sessions()
        .lock()
        .ok()
        .and_then(|map| map.values().next().map(|s| s.title.clone()));

    let pid = active_title
        .as_ref()
        .and(Some(std::process::id()))
        .unwrap_or_else(std::process::id);
    publish_shm_event_tagged(
        ShmEventType::ReplaySaveRequested,
        pid,
        0,
        0,
        active_title.as_deref().unwrap_or("OverlayClip"),
    );

    if let Some(bus) = DUKE_BUS.get() {
        bus.publish(DukeEvent::ReplaySaveRequested {
            game_title: active_title.clone(),
            source: "duke-launcher-overlay".to_string(),
        });
    }

    let capture_active = active_shm_apps().contains(&ShmSourceApp::DukeCapture);
    let message = if capture_active {
        "Instant Replay save signal transmitted to optional Duke Capture companion."
    } else {
        "Standalone mode: highlight marker logged locally (no companion daemon required)."
    };

    Ok(serde_json::json!({
        "dispatched": true,
        "standaloneFallback": !capture_active,
        "gameTitle": active_title,
        "message": message
    }))
}

#[tauri::command]
pub(crate) async fn purge_suite_ram_ipc(app: AppHandle) -> Result<Value, String> {
    let before_mb = get_process_working_set_mb();
    trim_process_working_set();
    let after_mb = get_process_working_set_mb();
    let freed_mb = (before_mb - after_mb).max(0.0);

    publish_shm_event(
        ShmEventType::RamPurge,
        std::process::id(),
        after_mb.to_bits(),
        0,
    );
    if let Some(bus) = DUKE_BUS.get() {
        bus.publish(DukeEvent::RamPurgeRequested {
            source: "duke-launcher-overlay".to_string(),
        });
    }

    let payload = serde_json::json!({
        "beforeMb": before_mb,
        "workingSetMb": after_mb,
        "freedMb": freed_mb,
        "status": "SUITE PURGED"
    });
    let _ = app.emit("memory-telemetry", &payload);
    Ok(payload)
}

pub(crate) fn spawn_overlay_trigger_watcher(app: AppHandle) {
    #[cfg(target_os = "windows")]
    std::thread::Builder::new()
        .name("duke-overlay-trigger".to_string())
        .spawn(move || {
            use windows::Win32::UI::Input::KeyboardAndMouse::{
                RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL,
                MOD_NOREPEAT, MOD_SHIFT,
            };
            use windows::Win32::UI::WindowsAndMessaging::{
                PeekMessageW, MSG, PM_REMOVE, WM_HOTKEY,
            };

            const HOTKEY_ID: i32 = 0x444B;
            const HOTKEY_VISION_ID: i32 = 0x444C;
            let registered = unsafe {
                RegisterHotKey(
                    None,
                    HOTKEY_ID,
                    HOT_KEY_MODIFIERS(MOD_ALT.0 | MOD_NOREPEAT.0),
                    0x47, // 'G'
                )
                .is_ok()
            };
            let vision_registered = unsafe {
                RegisterHotKey(
                    None,
                    HOTKEY_VISION_ID,
                    HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_SHIFT.0 | MOD_NOREPEAT.0),
                    0x54, // 'T' -> Ctrl+Shift+T triggers Duke Vision + Overlay
                )
                .is_ok()
            };

            let mut guide_was_down = false;
            let mut radial_chord_was_down = false;
            let mut vision_chord_was_down = false;
            let mut fse_chord_was_down = false;
            let mut guide_hold_start: Option<Instant> = None;
            let mut guide_combo_used = false;
            while !APP_SHUTDOWN.load(Ordering::Acquire) {
                let mut should_toggle_overlay = false;
                let mut should_toggle_guide = false;
                let mut should_toggle_radial = false;
                let mut should_vision = false;

                if fse_shell::poll_fse_guide_chord_edge(&mut fse_chord_was_down) {
                    should_toggle_overlay = true;
                }

                if registered || vision_registered {
                    let mut msg = MSG::default();
                    unsafe {
                        while PeekMessageW(&mut msg, None, WM_HOTKEY, WM_HOTKEY, PM_REMOVE).as_bool() {
                            if msg.message == WM_HOTKEY {
                                let id = msg.wParam.0 as i32;
                                if id == HOTKEY_ID {
                                    should_toggle_overlay = true;
                                } else if id == HOTKEY_VISION_ID {
                                    should_vision = true;
                                }
                            }
                        }
                    }
                }

                let pad = couch_mode::poll_universal_controller_state();
                let deck_active = COUCH_MODE_ACTIVE.load(Ordering::Acquire);
                let chord_active = desktop_chord::process_pad_for_desktop_chord(&pad, deck_active);
                let vision_chord_down = !chord_active && pad.connected && pad.guide && pad.y;
                let radial_chord_down =
                    pad.connected && pad.guide && pad.lb && pad.controller_kind != "edgetx";

                if pad.connected && pad.guide {
                    if !guide_was_down {
                        guide_hold_start = Some(Instant::now());
                        guide_combo_used = false;
                        if !chord_active && !pad.y && !pad.lb {
                            should_toggle_guide = true;
                            guide_combo_used = true;
                        }
                    }
                    let dz = pad.deadzone.clamp(0.05, 0.45);
                    if pad.lx.abs() > dz
                        || pad.ly.abs() > dz
                        || pad.ry.abs() > dz
                        || pad.rt > 0.45
                        || pad.lt > 0.45
                        || pad.a
                        || pad.b
                        || pad.x
                        || pad.y
                        || pad.lb
                        || pad.rb
                        || pad.up
                        || pad.down
                        || pad.ls
                    {
                        guide_combo_used = true;
                    }
                } else if guide_was_down {
                    if let Some(started) = guide_hold_start.take() {
                        if !guide_combo_used && started.elapsed() < Duration::from_millis(450) {
                            should_toggle_guide = true;
                        }
                    }
                    guide_combo_used = false;
                }

                if vision_chord_down && !vision_chord_was_down {
                    should_vision = true;
                    guide_combo_used = true;
                } else if radial_chord_down && !radial_chord_was_down {
                    should_toggle_radial = true;
                    guide_combo_used = true;
                }
                guide_was_down = pad.connected && pad.guide;
                radial_chord_was_down = radial_chord_down;
                vision_chord_was_down = vision_chord_down;

                if should_vision {
                    let app_vis = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = toggle_overlay_window(app_vis.clone(), None).await;
                        let _ = duke_vision::trigger_duke_vision_capture(
                            app_vis,
                            None,
                            Some("auto".to_string()),
                            Some("en".to_string()),
                            Some("both".to_string()),
                        )
                        .await;
                    });
                } else if should_toggle_radial {
                    couch_mode::handle_controller_guide_event(&app, "toggle_radial", &pad.controller_kind, None);
                } else if should_toggle_guide {
                    let app_guide = app.clone();
                    let guide_kind = pad.controller_kind.clone();
                    tauri::async_runtime::spawn(async move {
                        let main_visible = app_guide
                            .get_webview_window("main")
                            .map(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false))
                            .unwrap_or(false);
                        if main_visible {
                            couch_mode::handle_controller_guide_event(
                                &app_guide,
                                "toggle_couch_mode",
                                &guide_kind,
                                None,
                            );
                        } else {
                            let _ = toggle_overlay_window(app_guide, None).await;
                        }
                    });
                } else if should_toggle_overlay {
                    let app_toggle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = toggle_overlay_window(app_toggle, None).await;
                    });
                }

                std::thread::sleep(Duration::from_millis(if chord_active { 16 } else { 55 }));
            }
            #[allow(unreachable_code)]
            if registered {
                unsafe {
                    let _ = UnregisterHotKey(None, HOTKEY_ID);
                }
            }
            #[allow(unreachable_code)]
            if vision_registered {
                unsafe {
                    let _ = UnregisterHotKey(None, HOTKEY_VISION_ID);
                }
            }
        })
        .ok();

    #[cfg(not(target_os = "windows"))]
    let _ = app;
}

pub(crate) fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

pub(crate) fn write_pure_rust_zip_archive(
    zip_path: &Path,
    entries: &[(String, Vec<u8>)],
) -> Result<(), String> {
    let file = File::create(zip_path)
        .map_err(|e| format!("Failed to create backup archive '{}': {}", zip_path.display(), e))?;
    let mut writer = std::io::BufWriter::with_capacity(256 * 1024, file);

    struct CentralRecord {
        name_bytes: Vec<u8>,
        crc32: u32,
        size: u32,
        offset: u32,
    }

    let mut central_dir: Vec<CentralRecord> = Vec::with_capacity(entries.len());
    let mut current_offset: u32 = 0;

    for (rel_path, content) in entries {
        let normalized_name = rel_path.replace('\\', "/").trim_start_matches('/').to_string();
        let name_bytes = normalized_name.into_bytes();
        let size = u32::try_from(content.len()).unwrap_or(u32::MAX);
        let crc = crc32_ieee(content);
        let name_len = name_bytes.len() as u16;

        // Local file header (PK\x03\x04)
        let mut local_hdr = Vec::with_capacity(30 + name_bytes.len());
        local_hdr.extend_from_slice(&0x0403_4B50u32.to_le_bytes()); // signature
        local_hdr.extend_from_slice(&20u16.to_le_bytes()); // version needed (2.0)
        local_hdr.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 flag
        local_hdr.extend_from_slice(&0u16.to_le_bytes()); // compression method: 0 (STORE)
        local_hdr.extend_from_slice(&0u16.to_le_bytes()); // mod time
        local_hdr.extend_from_slice(&0x5421u16.to_le_bytes()); // mod date
        local_hdr.extend_from_slice(&crc.to_le_bytes()); // crc32
        local_hdr.extend_from_slice(&size.to_le_bytes()); // compressed size
        local_hdr.extend_from_slice(&size.to_le_bytes()); // uncompressed size
        local_hdr.extend_from_slice(&name_len.to_le_bytes()); // file name len
        local_hdr.extend_from_slice(&0u16.to_le_bytes()); // extra field len
        local_hdr.extend_from_slice(&name_bytes);

        writer
            .write_all(&local_hdr)
            .map_err(|e| format!("Failed writing ZIP local header: {}", e))?;
        writer
            .write_all(content)
            .map_err(|e| format!("Failed writing ZIP entry content: {}", e))?;

        central_dir.push(CentralRecord {
            name_bytes,
            crc32: crc,
            size,
            offset: current_offset,
        });

        current_offset = current_offset
            .saturating_add(local_hdr.len() as u32)
            .saturating_add(size);
    }

    let central_start = current_offset;
    let mut central_size: u32 = 0;

    for rec in &central_dir {
        let name_len = rec.name_bytes.len() as u16;
        let mut cd_hdr = Vec::with_capacity(46 + rec.name_bytes.len());
        cd_hdr.extend_from_slice(&0x0201_4B50u32.to_le_bytes()); // central dir signature
        cd_hdr.extend_from_slice(&20u16.to_le_bytes()); // version made by
        cd_hdr.extend_from_slice(&20u16.to_le_bytes()); // version needed
        cd_hdr.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 flag
        cd_hdr.extend_from_slice(&0u16.to_le_bytes()); // compression method: STORE
        cd_hdr.extend_from_slice(&0u16.to_le_bytes()); // mod time
        cd_hdr.extend_from_slice(&0x5421u16.to_le_bytes()); // mod date
        cd_hdr.extend_from_slice(&rec.crc32.to_le_bytes());
        cd_hdr.extend_from_slice(&rec.size.to_le_bytes());
        cd_hdr.extend_from_slice(&rec.size.to_le_bytes());
        cd_hdr.extend_from_slice(&name_len.to_le_bytes());
        cd_hdr.extend_from_slice(&0u16.to_le_bytes()); // extra len
        cd_hdr.extend_from_slice(&0u16.to_le_bytes()); // comment len
        cd_hdr.extend_from_slice(&0u16.to_le_bytes()); // disk number start
        cd_hdr.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        cd_hdr.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        cd_hdr.extend_from_slice(&rec.offset.to_le_bytes()); // local header offset
        cd_hdr.extend_from_slice(&rec.name_bytes);

        writer
            .write_all(&cd_hdr)
            .map_err(|e| format!("Failed writing ZIP central directory: {}", e))?;
        central_size = central_size.saturating_add(cd_hdr.len() as u32);
    }

    let entry_count = central_dir.len().min(u16::MAX as usize) as u16;
    let mut eocd = Vec::with_capacity(22);
    eocd.extend_from_slice(&0x0605_4B50u32.to_le_bytes()); // EOCD signature
    eocd.extend_from_slice(&0u16.to_le_bytes()); // disk number
    eocd.extend_from_slice(&0u16.to_le_bytes()); // central dir start disk
    eocd.extend_from_slice(&entry_count.to_le_bytes());
    eocd.extend_from_slice(&entry_count.to_le_bytes());
    eocd.extend_from_slice(&central_size.to_le_bytes());
    eocd.extend_from_slice(&central_start.to_le_bytes());
    eocd.extend_from_slice(&0u16.to_le_bytes()); // comment len

    writer
        .write_all(&eocd)
        .map_err(|e| format!("Failed writing ZIP EOCD record: {}", e))?;
    writer
        .flush()
        .map_err(|e| format!("Failed flushing ZIP archive: {}", e))?;

    Ok(())
}

#[tauri::command]
pub(crate) async fn backup_game_saves(
    _app: AppHandle,
    game_id: Option<String>,
    title: Option<String>,
    install_dir: Option<String>,
    exe_path: Option<String>,
) -> Result<String, String> {
    crate::ludusavi::backup_game_saves_core(game_id, title, install_dir, exe_path).await
}

pub(crate) fn shutdown_flush_downloads(app_handle: &AppHandle) {
    if let Ok(pause_map) = get_pause_map().lock() {
        for flag in pause_map.values() {
            flag.store(true, Ordering::Relaxed);
        }
    }
    let auto_clean = load_settings_from_disk(app_handle).auto_clean_completed;
    if let Ok(mut guard) = get_queue_store().lock() {
        for job in guard.iter_mut() {
            reconcile_job_bytes_from_disk(job);
            if job.status == "DOWNLOADING" || job.status == "RESOLVING" {
                job.status = "PAUSED".to_string();
                job.speed_mbps = 0.0;
                job.eta_secs = None;
                job.message = format!(
                    "Saved at {} - Click RESUME to continue",
                    format_bytes(job.downloaded_bytes)
                );
            }
        }
        prune_auto_cleanable_jobs(&mut guard, auto_clean);
        persist_queue_to_disk(app_handle, &guard);
    }
}

pub(crate) mod crash_guard {
    use core::ffi::c_void;
    use std::backtrace::Backtrace;
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    const EXCEPTION_STACK_OVERFLOW: u32 = 0xC000_00FD;
    const EXCEPTION_CONTINUE_SEARCH: i32 = 0;

    #[cfg(target_os = "windows")]
    #[repr(C)]
    #[allow(dead_code)]
    struct Win32ExceptionRecord {
        exception_code: u32,
        exception_flags: u32,
        exception_record: *const Win32ExceptionRecord,
        exception_address: *const c_void,
        number_parameters: u32,
    }

    #[cfg(target_os = "windows")]
    #[repr(C)]
    #[allow(dead_code)]
    struct Win32ExceptionPointers {
        exception_record: *const Win32ExceptionRecord,
        context_record: *const c_void,
    }

    #[cfg(target_os = "windows")]
    #[link(name = "kernel32")]
    extern "system" {
        fn SetUnhandledExceptionFilter(
            filter: Option<unsafe extern "system" fn(*const Win32ExceptionPointers) -> i32>,
        ) -> *const c_void;
    }

    fn crash_log_paths() -> Vec<PathBuf> {
        let mut paths = Vec::with_capacity(2);
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(dir) = exe_path.parent() {
                paths.push(dir.join("duke-launcher-crash.log"));
            }
        }
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            if !local_app_data.is_empty() {
                paths.push(
                    PathBuf::from(local_app_data)
                        .join("duke-launcher")
                        .join("crash.log"),
                );
            }
        }
        paths
    }

    fn write_crash_entry(context: &str, detail: &str, capture_backtrace: bool) {
        let epoch_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let pid = std::process::id();
        let current_thread = std::thread::current();
        let thread_name = current_thread.name().unwrap_or("<unnamed>");

        let mut report = format!(
            "=== Duke Launcher Crash Report ===\n\
             timestamp_unix: {epoch_secs}\n\
             version: {}\n\
             pid: {pid}\n\
             thread: {thread_name}\n\
             kind: {context}\n\
             detail: {detail}\n",
            env!("CARGO_PKG_VERSION")
        );

        if capture_backtrace {
            let bt = Backtrace::force_capture();
            report.push_str(&format!("backtrace:\n{bt}\n"));
        } else {
            report.push_str("backtrace: <skipped on stack overflow>\n");
        }
        report.push('\n');

        let _ = std::io::stderr().write_all(report.as_bytes());
        let _ = std::io::stderr().flush();

        for path in crash_log_paths() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
                let _ = file.write_all(report.as_bytes());
                let _ = file.flush();
            }
        }
    }

    pub fn record_fatal_error(context: &str, err: &str) {
        write_crash_entry(context, err, true);
    }

    #[cfg(target_os = "windows")]
    fn exception_code_name(code: u32) -> &'static str {
        match code {
            0xC000_0005 => "EXCEPTION_ACCESS_VIOLATION",
            0xC000_001D => "EXCEPTION_ILLEGAL_INSTRUCTION",
            0xC000_0094 => "EXCEPTION_INT_DIVIDE_BY_ZERO",
            0xC000_00FD => "EXCEPTION_STACK_OVERFLOW",
            0xC000_0409 => "STATUS_STACK_BUFFER_OVERRUN",
            0xE06D_7363 => "CPP_EH_EXCEPTION",
            _ => "WIN32_SEH_EXCEPTION",
        }
    }

    #[cfg(target_os = "windows")]
    unsafe extern "system" fn unhandled_exception_filter(
        info: *const Win32ExceptionPointers,
    ) -> i32 {
        let (code, flags, addr) = if !info.is_null() && !(*info).exception_record.is_null() {
            let rec = &*(*info).exception_record;
            (
                rec.exception_code,
                rec.exception_flags,
                rec.exception_address as usize,
            )
        } else {
            (0, 0, 0)
        };
        let label = exception_code_name(code);
        let detail = format!(
            "code=0x{code:08X} ({label}) flags=0x{flags:08X} address=0x{addr:016X}"
        );
        write_crash_entry(
            "unhandled_win32_exception",
            &detail,
            code != EXCEPTION_STACK_OVERFLOW,
        );
        EXCEPTION_CONTINUE_SEARCH
    }

    pub fn install() {
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            let payload = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
                s.clone()
            } else {
                "non-string panic payload".to_string()
            };
            let location = panic_info
                .location()
                .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
                .unwrap_or_else(|| "<unknown location>".to_string());
            let detail = format!("at {location}: {payload}");
            write_crash_entry("rust_panic", &detail, true);
            prev_hook(panic_info);
        }));

        #[cfg(target_os = "windows")]
        unsafe {
            let _ = SetUnhandledExceptionFilter(Some(unhandled_exception_filter));
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemRuntimeDiagnostics {
    pub ioring: duke_core::nvme_prefetch::IoRingCapability,
    pub frame_pacing: duke_core::win32_ffi::FramePacingStatus,
    pub gpu_hardware: shader_swarm::GpuHardwareIdentity,
    pub projfs: projfs_stream::ProjFsRuntimeStatus,
    pub quick_resume: quick_resume::QuickResumeStateSummary,
    pub steam_cm: steam_cm::SteamCmSessionStatus,
    pub store_direct: store_direct::StoreDirectStatus,
}

#[tauri::command]
pub(crate) async fn get_frame_pacing_and_ioring_status() -> Result<SystemRuntimeDiagnostics, String> {
    let (ioring, frame_pacing, gpu_hardware, projfs, quick_resume) =
        run_blocking_timeout(10, "get_frame_pacing_and_ioring_status", || {
            Ok((
                duke_core::nvme_prefetch::probe_ioring_support(),
                duke_core::win32_ffi::inspect_frame_pacing_status(),
                shader_swarm::detect_gpu_hardware_identity(),
                projfs_stream::probe_projfs_status(),
                quick_resume::build_summary_snapshot(),
            ))
        })
        .await?;
    Ok(SystemRuntimeDiagnostics {
        ioring,
        frame_pacing,
        gpu_hardware,
        projfs,
        quick_resume,
        steam_cm: steam_cm::get_steam_cm_status().await,
        store_direct: store_direct::get_store_direct_status().await,
    })
}

#[tauri::command]
pub(crate) async fn toggle_half_ms_frame_pacing(
    enabled: bool,
) -> Result<duke_core::win32_ffi::FramePacingStatus, String> {
    run_blocking_timeout(5, "toggle_half_ms_frame_pacing", move || {
        if enabled {
            Ok(duke_core::win32_ffi::begin_frame_pacing_runtime())
        } else {
            Ok(duke_core::win32_ffi::end_frame_pacing_runtime())
        }
    })
    .await
}

#[tauri::command]
pub(crate) async fn purge_stale_driver_shaders() -> Result<duke_core::win32_ffi::ShaderCachePurgeReport, String> {
    run_blocking_timeout(30, "purge_stale_driver_shaders", || {
        let app_root = get_app_data_file_standalone("shader_swarm")?
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(std::env::temp_dir);
        let drv = duke_core::win32_ffi::detect_primary_gpu_driver_version();
        duke_core::win32_ffi::check_and_purge_stale_vendor_shader_caches(&app_root, drv.as_deref())
    })
    .await
}

pub(crate) fn init_duke_bus(bus_app: AppHandle) {
    let _ = DUKE_BUS.get_or_init(|| {
        DukeBus::start("duke-launcher", move |event| match event {
            DukeEvent::RamPurgeRequested { .. } => {
                duke_core::win32_ffi::trim_current_working_set();
                publish_shm_event(
                    ShmEventType::RamPurge,
                    std::process::id(),
                    get_process_working_set_mb().to_bits(),
                    0,
                );
            }
            DukeEvent::CaptureSignalTelemetry {
                system_peak_pct,
                motion_score,
                transient_spike,
                ..
            } => {
                duke_director::on_capture_telemetry_pulse(
                    &bus_app,
                    system_peak_pct,
                    motion_score,
                    transient_spike,
                );
            }
            DukeEvent::AchievementUnlocked {
                game_title,
                title_slug,
                ..
            } => {
                duke_director::on_achievement_unlocked_pulse(&bus_app, &game_title, &title_slug);
            }
            DukeEvent::ClipSaved {
                game_title,
                file_path,
                duration_secs,
            } => {
                duke_director::on_clip_saved_pulse(&game_title, &file_path, duration_secs);
            }
            DukeEvent::ControllerGuideAction {
                action,
                controller_kind,
                radial_slot,
                ..
            } => {
                couch_mode::handle_controller_guide_event(
                    &bus_app,
                    &action,
                    &controller_kind,
                    radial_slot,
                );
            }
            _ => {}
        })
    });
}

pub(crate) fn spawn_idle_ram_trim_watcher(app_handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(900)).await;
        if !APP_SHUTDOWN.load(Ordering::Acquire) {
            let ram_mb = get_process_working_set_mb();
            let doh_hosts = duke_core::net_doh::cached_doh_host_count();
            let _ = app_handle.emit(
                "memory-telemetry",
                serde_json::json!({
                    "workingSetMb": ram_mb,
                    "dohShield": if duke_core::net_doh::is_doh_enabled() { "ACTIVE" } else { "SYSTEM DNS" },
                    "dohCachedHosts": doh_hosts,
                    "portableMode": resolve_portable_data_dir().is_some(),
                    "status": "IDLE TRIMMED"
                }),
            );
        }
        let mut tick_count: u64 = 0;
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        loop {
            interval.tick().await;
            if APP_SHUTDOWN.load(Ordering::Acquire) {
                break;
            }
            tick_count = tick_count.saturating_add(1);
            let _ = duke_ai_rt::unload_idle_workers_internal(false);
            let ram_mode = RAM_TRIM_MODE.load(Ordering::Relaxed);
            let any_downloading = {
                let guard = get_queue_store().lock().unwrap_or_else(|p| p.into_inner());
                guard.iter().any(|j| {
                    j.status == "DOWNLOADING" || j.status == "RESOLVING" || j.status == "INSTALLING"
                })
            };
            let is_headless_or_minimized = app_handle
                .get_webview_window("main")
                .map(|w| w.is_minimized().unwrap_or(false))
                .unwrap_or(true);

            let should_trim = match ram_mode {
                0 => false,
                2 => !any_downloading || is_headless_or_minimized,
                _ => tick_count.is_multiple_of(4) && (!any_downloading || is_headless_or_minimized),
            };
            if should_trim {
                if ram_mode == 2 && is_headless_or_minimized {
                    duke_core::win32_ffi::enable_idle_eco_qos_mode();
                }
                trim_process_working_set();
            }
            if !(ram_mode == 2 || tick_count.is_multiple_of(4)) {
                continue;
            }

            let ram_mb = get_process_working_set_mb();
            let doh_hosts = duke_core::net_doh::cached_doh_host_count();
            publish_shm_event(
                ShmEventType::TelemetryTick,
                std::process::id(),
                ram_mb.to_bits(),
                doh_hosts as u64,
            );
            let _ = app_handle.emit(
                "memory-telemetry",
                serde_json::json!({
                    "workingSetMb": ram_mb,
                    "dohShield": if duke_core::net_doh::is_doh_enabled() { "ACTIVE" } else { "SYSTEM DNS" },
                    "dohCachedHosts": doh_hosts,
                    "portableMode": resolve_portable_data_dir().is_some(),
                    "status": if ram_mode == 0 {
                        "TRIM PAUSED"
                    } else if is_headless_or_minimized {
                        "TRAY DAEMON IDLE"
                    } else {
                        "IDLE TRIMMED"
                    }
                }),
            );
        }
    });
}

pub(crate) fn shutdown_all_runtime_services(app_handle: &AppHandle) {
    APP_SHUTDOWN.store(true, Ordering::Release);
    HOTKEY_WATCHER_ACTIVE.store(false, Ordering::Release);
    couch_mode::clear_transient_couch_state();
    save_watcher::stop_all_save_watchers();
    discord_ipc::clear_discord_presence();
    desktop_chord::set_desktop_chord_enabled(false);
    if let Some(cell) = DUKE_SHM_BUS.get() {
        if let Ok(guard) = cell.lock() {
            if let Some(ref bus) = *guard {
                bus.unregister_app();
            }
        }
    }
    shutdown_flush_downloads(app_handle);
    if let Ok(data_dir) = resolve_data_root_dir(app_handle) {
        self_heal::clear_session_lock_in_dir(&data_dir);
    }
}

#[tauri::command]
pub(crate) async fn get_self_heal_diagnostics(
    app: AppHandle,
) -> Result<self_heal::SelfHealDiagnostics, String> {
    run_blocking_timeout(5, "get_self_heal_diagnostics", move || {
        let data_dir = resolve_data_root_dir(&app)?;
        Ok(self_heal::get_self_heal_status(&data_dir))
    })
    .await
}

#[tauri::command]
pub(crate) async fn reset_self_heal_subsystem(
    app: AppHandle,
    name: String,
) -> Result<bool, String> {
    run_blocking_timeout(5, "reset_self_heal_subsystem", move || {
        let data_dir = resolve_data_root_dir(&app)?;
        self_heal::reset_disabled_subsystem(&data_dir, &name)
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_discord_presence_status() -> Result<discord_ipc::DiscordPresenceStatus, String> {
    run_blocking_timeout(5, "get_discord_presence_status", || {
        Ok(discord_ipc::get_discord_presence_status())
    })
    .await
}

#[tauri::command]
pub(crate) async fn set_discord_presence_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<discord_ipc::DiscordPresenceStatus, String> {
    run_blocking_timeout(5, "set_discord_presence_enabled", move || {
        let mut settings = load_settings_from_disk(&app);
        settings.discord_rpc_enabled = enabled;
        save_settings_to_disk(&app, &settings)?;
        apply_runtime_settings(&settings);
        if !enabled {
            discord_ipc::clear_discord_presence();
        } else if let Some(active_title) = get_active_sessions()
            .lock()
            .ok()
            .and_then(|guard| guard.values().next().map(|s| s.title.clone()))
        {
            discord_ipc::set_discord_presence_game(&active_title, "PC Windows", None, None);
        }
        Ok(discord_ipc::get_discord_presence_status())
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_save_watcher_status() -> Result<save_watcher::SaveWatcherStatus, String> {
    run_blocking_timeout(5, "get_save_watcher_status", || {
        Ok(save_watcher::get_save_watcher_status())
    })
    .await
}

#[tauri::command]
pub(crate) async fn start_save_watcher(
    app: AppHandle,
    game_id: String,
) -> Result<save_watcher::SaveWatcherStatus, String> {
    run_blocking_timeout(10, "start_save_watcher", move || {
        let library = load_library_from_disk(&app)?;
        let game = library
            .into_iter()
            .find(|g| g.id == game_id)
            .ok_or_else(|| format!("Game not found in library: {game_id}"))?;
        save_watcher::start_save_watcher_for_game(Some(app), &game)
    })
    .await
}

#[tauri::command]
pub(crate) async fn stop_save_watcher(
    game_id: Option<String>,
) -> Result<save_watcher::SaveWatcherStatus, String> {
    run_blocking_timeout(5, "stop_save_watcher", move || {
        match game_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(gid) => save_watcher::stop_save_watcher_for_game(gid),
            None => save_watcher::stop_all_save_watchers(),
        }
        Ok(save_watcher::get_save_watcher_status())
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_desktop_chord_status() -> Result<desktop_chord::DesktopChordStatus, String> {
    run_blocking_timeout(5, "get_desktop_chord_status", || {
        Ok(desktop_chord::get_desktop_chord_status())
    })
    .await
}

#[tauri::command]
pub(crate) async fn set_desktop_chord_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<desktop_chord::DesktopChordStatus, String> {
    run_blocking_timeout(5, "set_desktop_chord_enabled", move || {
        let mut settings = load_settings_from_disk(&app);
        settings.desktop_chord_enabled = enabled;
        save_settings_to_disk(&app, &settings)?;
        apply_runtime_settings(&settings);
        Ok(desktop_chord::get_desktop_chord_status())
    })
    .await
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn test_clean_steam_clan_tokens_and_bbcode() {
        let raw = "[h2]Patch 1.4[/h2]\n{STEAM_CLAN_IMAGE}/39049601/abc123.png\n[img]{STEAM_CLAN_LOGO_IMAGE}/42/banner.jpg[/img]\n[list][*]Fixed [b]save sync[/b] and [i]co-op[/i]\n[*]Read [url=https://store.steampowered.com]Release Notes[/url][/list]";
        let cleaned = clean_steam_clan_tokens(raw);

        assert!(!cleaned.contains("STEAM_CLAN_IMAGE"));
        assert!(!cleaned.contains("STEAM_CLAN_LOGO_IMAGE"));
        assert!(!cleaned.contains("[img]"));
        assert!(cleaned.contains("[h2]Patch 1.4[/h2]"));
        assert!(cleaned.contains("[b]save sync[/b]"));
        assert!(cleaned.contains("[i]co-op[/i]"));
        assert!(cleaned.contains("[list]"));
        assert!(cleaned.contains("[*]Fixed"));
        assert!(cleaned.contains("[url=https://store.steampowered.com]Release Notes[/url]"));

        let html_raw = "<h3>Hotfix</h3><p>Improved <strong>FPS</strong> &amp; stability.</p><ul><li>Added LAN</li></ul>";
        let html_cleaned = clean_steam_clan_tokens(html_raw);
        assert!(html_cleaned.contains("[h3]Hotfix[/h3]"));
        assert!(html_cleaned.contains("[b]FPS[/b] & stability."));
        assert!(html_cleaned.contains("[*]Added LAN"));
    }

    #[test]
    fn test_hero_candidate_priority_order() {
        let urls = build_steam_hero_candidates(1245620, None);
        assert_eq!(urls.len(), 3);
        assert_eq!(
            urls[0],
            "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/1245620/library_hero.jpg"
        );
        assert_eq!(
            urls[1],
            "https://cdn.cloudflare.steamstatic.com/steam/apps/1245620/library_hero.jpg"
        );
        assert_eq!(
            urls[2],
            "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/1245620/header.jpg"
        );
    }

    #[test]
    fn test_auto_clean_job_eligibility_and_pruning() {
        let mut jobs = vec![
            DownloadJob {
                download_id: "dl_registered".to_string(),
                status: "COMPLETED".to_string(),
                registered_in_library: true,
                verdict: Some("CLEAN".to_string()),
                ..Default::default()
            },
            DownloadJob {
                download_id: "dl_danger".to_string(),
                status: "COMPLETED".to_string(),
                registered_in_library: false,
                verdict: Some("DANGER".to_string()),
                ..Default::default()
            },
            DownloadJob {
                download_id: "dl_active".to_string(),
                status: "DOWNLOADING".to_string(),
                registered_in_library: false,
                verdict: None,
                ..Default::default()
            },
        ];

        assert!(is_job_auto_cleanable(&jobs[0]));
        assert!(!is_job_auto_cleanable(&jobs[1]));
        assert!(!is_job_auto_cleanable(&jobs[2]));

        assert!(!prune_auto_cleanable_jobs(&mut jobs, false));
        assert_eq!(jobs.len(), 3);

        assert!(prune_auto_cleanable_jobs(&mut jobs, true));
        assert_eq!(jobs.len(), 2);
        assert!(jobs.iter().any(|j| j.download_id == "dl_danger"));
        assert!(jobs.iter().any(|j| j.download_id == "dl_active"));
    }

    #[test]
    fn test_atomic_write_and_default_serde_resilience() {
        let defaults: LauncherSettings = serde_json::from_str("{}").unwrap();
        assert!(defaults.auto_clean_completed);
        assert_eq!(defaults.download_speed_preset, "max");
        assert!(!defaults.compact_library_cards);
        assert!(defaults.performance_boost_enabled);
        assert!(!defaults.auto_backup_saves_on_exit);
        assert_eq!(defaults.cpu_priority_mode, "auto");
        assert_eq!(defaults.ram_trim_mode, "standard");
        assert!(!defaults.auto_freeze_minimized);
        assert_eq!(defaults.nvme_cache_budget_pct, 25);
        assert!(defaults.doh_enabled);
        assert!(defaults.custom_proxy_url.is_none());
        assert_eq!(defaults.custom_speed_cap_mbps, 0.0);
        assert!(defaults.auto_unblock_motw);
        assert!(!defaults.portable_sandbox_default);
        assert_eq!(defaults.max_restore_points, 5);
        assert!(defaults.block_game_telemetry);
        assert!(defaults.global_launch_args.is_none());
        assert!(!defaults.swarm_auto_sync_saves);
        assert!(!defaults.start_minimized_with_windows);
        assert!(!defaults.minimize_to_tray);
        assert!(!defaults.close_to_tray);
        assert!(defaults.auto_check_updates);

        let partial_settings: LauncherSettings = serde_json::from_str(
            r#"{"downloadSpeedPreset":"balanced","nvmeCacheBudgetPct":40,"customProxyUrl":"socks5://127.0.0.1:1080","maxRestorePoints":10}"#,
        )
        .unwrap();
        assert_eq!(partial_settings.download_speed_preset, "balanced");
        assert_eq!(partial_settings.nvme_cache_budget_pct, 40);
        assert_eq!(
            partial_settings.custom_proxy_url.as_deref(),
            Some("socks5://127.0.0.1:1080")
        );
        assert_eq!(partial_settings.max_restore_points, 10);
        assert!(partial_settings.performance_boost_enabled);

        let partial_job: DownloadJob =
            serde_json::from_str(r#"{"downloadId":"dl_1","status":"PAUSED"}"#).unwrap();
        assert_eq!(partial_job.download_id, "dl_1");
        assert!(!partial_job.registered_in_library);

        let dir = std::env::temp_dir().join(format!("duke_launcher_test_{}", std::process::id()));
        let target = dir.join("atomic_test.json");
        atomic_write_string(&target, "{\"ok\":1}").unwrap();
        atomic_write_string(&target, "{\"ok\":2}").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "{\"ok\":2}");
        let leftover_tmps: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftover_tmps.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_catalog_value_score_and_online_pvp_classification() {
        let mut paid_sp = CatalogGameEntry {
            app_id: 10,
            name: "Paid Singleplayer Gem".to_string(),
            price_cents: 5999,
            initial_price_cents: 5999,
            ccu: 25000,
            owners_min: 2_000_000,
            owners_max: 5_000_000,
            positive: 95000,
            negative: 5000,
            is_singleplayer: true,
            is_coop: true,
            genres: vec!["RPG".to_string()],
            ..Default::default()
        };
        finalize_catalog_entry(&mut paid_sp, 1000);
        assert!(!paid_sp.is_online_only_pvp);
        assert_eq!(paid_sp.owners_label, "2.0M - 5.0M");

        let mut free_pvp = CatalogGameEntry {
            app_id: 20,
            name: "Free Online Arena".to_string(),
            price_cents: 0,
            initial_price_cents: 0,
            ccu: 50000,
            positive: 60000,
            negative: 40000,
            genres: vec!["Massively Multiplayer".to_string()],
            ..Default::default()
        };
        finalize_catalog_entry(&mut free_pvp, 1000);
        assert!(free_pvp.is_online_only_pvp);
        assert!(paid_sp.value_score > free_pvp.value_score);

        let parsed_cents = json_val_to_u32(Some(&serde_json::json!("1999")));
        assert_eq!(parsed_cents, 1999);
        assert_eq!(
            parse_steamspy_owners_band("1,000,000 .. 2,000,000"),
            (1_000_000, 2_000_000)
        );
    }

    #[test]
    fn test_duke_protocol_url_parser_and_clipboard_setting_default() {
        let defaults: LauncherSettings = serde_json::from_str("{}").unwrap();
        assert!(defaults.auto_detect_clipboard);

        assert_eq!(
            parse_duke_protocol_download_url(
                "duke://download?url=https%3A%2F%2Fbuzzheavier.com%2Fabc123%3Fv%3D1%2B2"
            ),
            Some("https://buzzheavier.com/abc123?v=1+2".to_string())
        );
        assert_eq!(
            parse_duke_protocol_download_url(
                "\"DUKE://download/?src=bar&url=http%3A%2F%2Fexample.com%2Farchive.zip\""
            ),
            Some("http://example.com/archive.zip".to_string())
        );
        assert_eq!(
            parse_duke_protocol_download_url("duke://download?url=javascript%3Aalert(1)"),
            None
        );
        assert_eq!(
            parse_duke_protocol_download_url("duke://other?url=https%3A%2F%2Fexample.com%2Fa.zip"),
            None
        );
        assert_eq!(
            parse_duke_protocol_download_url("https://example.com/a.zip"),
            None
        );
    }

    #[test]
    fn test_aimd_chunk_scaling_work_stealing_and_seek_write() {
        let mut size = AIMD_MIN_CHUNK_BYTES;
        assert_eq!(size, 4 * 1024 * 1024);

        // Additive Increase (+4 MiB) up to 64 MiB cap on fast sockets
        for _ in 0..20 {
            size = aimd_next_chunk_bytes(size, 0.5, 8 * 1024 * 1024, false);
        }
        assert_eq!(size, AIMD_MAX_CHUNK_BYTES);

        // Multiplicative Decrease (halving) on stall/jitter down to 4 MiB floor
        size = aimd_next_chunk_bytes(size, 1.0, 0, true);
        assert_eq!(size, 32 * 1024 * 1024);
        size = aimd_next_chunk_bytes(size, 1.0, 0, true);
        assert_eq!(size, 16 * 1024 * 1024);
        for _ in 0..5 {
            size = aimd_next_chunk_bytes(size, 1.0, 0, true);
        }
        assert_eq!(size, AIMD_MIN_CHUNK_BYTES);

        // Tail-latency work-stealing across atomic slots
        let slots: Vec<AtomicSegmentSlot> = (0..8)
            .map(|i| AtomicSegmentSlot::new(0, 0, 0, i as u64))
            .collect();
        let active_cnt = AtomicU64::new(2);
        let steal_lock = Mutex::new(());

        // Slot 0 completed, Slot 1 still has 32 MiB remaining
        slots[0].start.store(0, Ordering::Release);
        slots[0].current.store(4 * 1024 * 1024, Ordering::Release);
        slots[0].end.store(4 * 1024 * 1024 - 1, Ordering::Release);

        let v_start = 4 * 1024 * 1024;
        let v_end = 36 * 1024 * 1024 - 1;
        slots[1].start.store(v_start, Ordering::Release);
        slots[1].current.store(v_start, Ordering::Release);
        slots[1].end.store(v_end, Ordering::Release);

        let stolen = try_steal_slowest_tail(&slots, &active_cnt, &steal_lock).unwrap();
        assert_eq!(stolen, 2);
        assert_eq!(active_cnt.load(Ordering::Acquire), 3);
        let new_victim_end = slots[1].end.load(Ordering::Acquire);
        let stolen_start = slots[2].start.load(Ordering::Acquire);
        let stolen_end = slots[2].end.load(Ordering::Acquire);
        assert_eq!(stolen_start, new_victim_end + 1);
        assert_eq!(stolen_end, v_end);
        assert_eq!(stolen_start & 4095, 0);

        // Pre-allocated positional offset `seek_write` test
        let dir = std::env::temp_dir().join(format!("duke_aimd_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let part_p = dir.join("prealloc.part");
        let file = Arc::new(
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(&part_p)
                .unwrap(),
        );
        file.set_len(128).unwrap();
        let f2 = Arc::clone(&file);
        let t1 = std::thread::spawn(move || {
            let right = vec![0xBBu8; 64];
            seek_write_all_at(&f2, 64, &right).unwrap();
        });
        let left = vec![0xAAu8; 64];
        seek_write_all_at(&file, 0, &left).unwrap();
        t1.join().unwrap();
        file.sync_all().unwrap();
        drop(file);

        let written = fs::read(&part_p).unwrap();
        assert_eq!(written.len(), 128);
        assert!(written[..64].iter().all(|&b| b == 0xAA));
        assert!(written[64..].iter().all(|&b| b == 0xBB));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_shannon_entropy_and_pe_header_auditor() {
        let mut zero_hist = [0u64; 256];
        zero_hist[0] = 4096;
        assert!((shannon_entropy_from_histogram(&zero_hist, 4096) - 0.0).abs() < 1e-6);

        let uniform_hist = [16u64; 256];
        assert!((shannon_entropy_from_histogram(&uniform_hist, 4096) - 8.0).abs() < 1e-6);

        // Build minimal synthetic 64-bit PE with `.text` and `UPX0` section headers
        let dir = std::env::temp_dir().join(format!("duke_pe_audit_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let pe_path = dir.join("packed_sample.exe");
        let mut pe_bytes = vec![0u8; 4096];
        pe_bytes[0] = b'M';
        pe_bytes[1] = b'Z';
        pe_bytes[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        pe_bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        pe_bytes[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes()); // AMD64
        pe_bytes[0x86..0x88].copy_from_slice(&2u16.to_le_bytes()); // 2 sections
        pe_bytes[0x94..0x96].copy_from_slice(&0xF0u16.to_le_bytes()); // OptionalHeader size (240)
        pe_bytes[0x96..0x98].copy_from_slice(&0x0022u16.to_le_bytes()); // EXECUTABLE_IMAGE
        pe_bytes[0x98..0x9A].copy_from_slice(&0x020Bu16.to_le_bytes()); // PE32+
        pe_bytes[0x98 + 68..0x98 + 70].copy_from_slice(&2u16.to_le_bytes()); // GUI subsystem
        let sec_base = 0x80 + 24 + 0xF0;
        pe_bytes[sec_base..sec_base + 5].copy_from_slice(b".text");
        pe_bytes[sec_base + 40..sec_base + 44].copy_from_slice(b"UPX0");
        fs::write(&pe_path, &pe_bytes).unwrap();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let report = rt
            .block_on(scan_file_security_core(pe_path.to_string_lossy().to_string()))
            .unwrap();
        assert_eq!(report.verdict, "DANGER");
        assert_eq!(report.pe_architecture.as_deref(), Some("x64 (AMD64) EXE"));
        assert_eq!(report.pe_sections, vec![".text".to_string(), "UPX0".to_string()]);
        assert!(report
            .details
            .iter()
            .any(|d| d.contains("Shannon Entropy:") && d.contains("bits/byte")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_apply_window_backdrop_attributes() {
        assert_eq!(
            resolve_backdrop_type_for_theme("acrylic"),
            DWMSBT_TRANSIENTWINDOW
        );
        for solid in [
            "dark",
            "light",
            "minecraft",
            "sunset",
            "aerospace",
            "cyberpunk",
            "terminal",
            "crimson",
            "ocean",
            "frost",
        ] {
            assert_eq!(resolve_backdrop_type_for_theme(solid), DWMSBT_NONE);
        }

        // Invalid HWND guard
        let err = apply_backdrop_with(0, "acrylic", |_, _, _| 0, |_, _| 0).unwrap_err();
        assert!(err.contains("HWND"));

        // Verify exact Win32 DwmSetWindowAttribute + margin values for translucent overlay backdrop
        let mut attrs = Vec::new();
        let mut margins_log = Vec::new();
        let chosen = apply_backdrop_with(
            0x1234isize,
            "acrylic",
            |_, attr, val| {
                attrs.push((attr, val));
                0
            },
            |_, m| {
                margins_log.push(m);
                0
            },
        )
        .unwrap();
        assert_eq!(chosen, DWMSBT_TRANSIENTWINDOW);
        assert_eq!(
            attrs,
            vec![
                (DWMWA_USE_IMMERSIVE_DARK_MODE, 1),
                (DWMWA_SYSTEMBACKDROP_TYPE, DWMSBT_TRANSIENTWINDOW),
            ]
        );
        assert_eq!(
            margins_log,
            vec![DwmMargins {
                cx_left_width: -1,
                cx_right_width: -1,
                cy_top_height: -1,
                cy_bottom_height: -1,
            }]
        );

        // Verify opaque reset for solid themes
        attrs.clear();
        margins_log.clear();
        let solid_chosen = apply_backdrop_with(
            0x1234isize,
            "dark",
            |_, attr, val| {
                attrs.push((attr, val));
                0
            },
            |_, m| {
                margins_log.push(m);
                0
            },
        )
        .unwrap();
        assert_eq!(solid_chosen, DWMSBT_NONE);
        assert_eq!(
            attrs,
            vec![
                (DWMWA_USE_IMMERSIVE_DARK_MODE, 1),
                (DWMWA_SYSTEMBACKDROP_TYPE, DWMSBT_NONE),
            ]
        );
        assert_eq!(
            margins_log,
            vec![DwmMargins {
                cx_left_width: 0,
                cx_right_width: 0,
                cy_top_height: 0,
                cy_bottom_height: 0,
            }]
        );
    }

    #[test]
    fn test_money_saved_scoped_to_turbo_downloader() {
        let lib = vec![
            GameEntry {
                id: "dl_1".to_string(),
                title: "Downloaded Game".to_string(),
                acquisition_source: "turbo_downloader".to_string(),
                steam_price_cents: Some(5999),
                ..Default::default()
            },
            GameEntry {
                id: "steam_1".to_string(),
                title: "Imported Steam Game".to_string(),
                acquisition_source: "steam".to_string(),
                steam_price_cents: Some(6999),
                ..Default::default()
            },
            GameEntry {
                id: "manual_1".to_string(),
                title: "Manual Game".to_string(),
                acquisition_source: "manual".to_string(),
                steam_price_cents: Some(3999),
                ..Default::default()
            },
        ];
        assert_eq!(compute_money_saved_cents(&lib), 5999);
    }

    #[test]
    fn test_normalize_migrates_playtime_and_version_comparison() {
        let mut legacy = GameEntry {
            id: "old_1".to_string(),
            title: "Legacy Entry".to_string(),
            source: "Steam".to_string(),
            playtime_seconds: 5400,
            ..Default::default()
        };
        normalize_game_entry(&mut legacy);
        assert_eq!(legacy.acquisition_source, "steam");
        assert_eq!(legacy.duke_playtime_secs, 5400);
        assert_eq!(legacy.imported_playtime_secs, 0);
        assert_eq!(legacy.playtime_seconds, 5400);

        assert!(is_newer_version("1.0.9", "v1.0.10"));
        assert!(is_newer_version("", "0.1.0"));
        assert!(!is_newer_version("v2.1.0", "2.1.0"));
        assert!(!is_newer_version("2.1.0", "2.0.9"));
    }

    #[test]
    fn test_folder_exe_scoring_penalizes_helpers_and_honors_depth_limit() {
        let root = std::env::temp_dir().join(format!(
            "duke_exe_score_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let game_dir = root.join("Elden Ring");
        let bin_dir = game_dir.join("Game");
        let deep_dir = game_dir.join("a").join("b").join("c"); // depth 4 relative to game_dir
        fs::create_dir_all(&bin_dir).unwrap();
        fs::create_dir_all(&deep_dir).unwrap();

        let helper_names = [
            "unins000.exe",
            "UnityCrashHandler64.exe",
            "CrashReportClient.exe",
            "vc_redist.x64.exe",
            "dxsetup.exe",
            "dotnetfx48.exe",
        ];
        for name in helper_names {
            let helper_path = game_dir.join(name);
            fs::write(&helper_path, vec![0u8; 512 * 1024]).unwrap();
            let s = score_exe_candidate(&helper_path, 512 * 1024, "Elden Ring", &["elden".into(), "ring".into()]);
            assert!(s < 0, "expected negative score for helper {}", name);
        }

        let launcher_exe = bin_dir.join("start_protected_game.exe");
        let main_exe = bin_dir.join("eldenring.exe");
        let too_deep_exe = deep_dir.join("elden_ring_deep_archive_loader.exe");
        fs::write(&launcher_exe, vec![0u8; 220 * 1024]).unwrap();
        fs::write(&main_exe, vec![0u8; 400 * 1024]).unwrap();
        fs::write(&too_deep_exe, vec![0u8; 900 * 1024]).unwrap();

        let picked = find_best_game_exe_with_max_depth(&game_dir, "Elden Ring", 3)
            .expect("should pick main executable within 3 levels");
        assert_eq!(picked, main_exe);

        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_win32_lnk_resolution_and_desktop_cleanup() {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::{Interface, PCWSTR};
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED,
        };
        use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

        fn create_test_lnk(
            lnk_path: &Path,
            target: &Path,
            args: &str,
            workdir: &Path,
        ) {
            let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            let shell_link: IShellLinkW =
                unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.unwrap();
            let t_w: Vec<u16> = target
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let a_w: Vec<u16> = args.encode_utf16().chain(std::iter::once(0)).collect();
            let w_w: Vec<u16> = workdir
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let l_w: Vec<u16> = lnk_path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            unsafe {
                shell_link.SetPath(PCWSTR(t_w.as_ptr())).unwrap();
                shell_link.SetArguments(PCWSTR(a_w.as_ptr())).unwrap();
                shell_link.SetWorkingDirectory(PCWSTR(w_w.as_ptr())).unwrap();
                let pf: IPersistFile = shell_link.cast().unwrap();
                pf.Save(PCWSTR(l_w.as_ptr()), true).unwrap();
                if hr.is_ok() {
                    CoUninitialize();
                }
            }
        }

        let root = std::env::temp_dir().join(format!(
            "duke_lnk_desktop_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let fake_desktop = root.join("Desktop");
        let backup_dir = fake_desktop.join("Duke Shortcut Backup");
        let game_dir = root.join("Games").join("SpaceQuest");
        fs::create_dir_all(&fake_desktop).unwrap();
        fs::create_dir_all(&game_dir).unwrap();

        let game_exe = game_dir.join("SpaceQuest.exe");
        fs::write(&game_exe, vec![0u8; 256 * 1024]).unwrap();

        let game_lnk = fake_desktop.join("Space Quest.lnk");
        create_test_lnk(&game_lnk, &game_exe, "-novid -windowed", &game_dir);

        let browser_exe = game_dir.join("chrome.exe");
        fs::write(&browser_exe, vec![0u8; 256 * 1024]).unwrap();
        let browser_lnk = fake_desktop.join("Google Chrome.lnk");
        create_test_lnk(&browser_lnk, &browser_exe, "", &game_dir);

        let resolved = resolve_lnk_shortcut(&game_lnk).expect("IShellLinkW should resolve .lnk");
        assert!(resolved.target_exe.eq_ignore_ascii_case(&game_exe.to_string_lossy()));
        assert_eq!(resolved.arguments.as_deref(), Some("-novid -windowed"));
        assert_eq!(
            resolved.working_dir.as_deref(),
            Some(game_dir.to_string_lossy().as_ref())
        );

        let (scanned, skipped_non_game, candidates) =
            collect_desktop_game_shortcuts_sync(std::slice::from_ref(&fake_desktop));
        assert_eq!(scanned, 2);
        assert_eq!(skipped_non_game, 1);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].title, "Space Quest");

        let moved = move_file_with_unique_name(&candidates[0].lnk_path, &backup_dir).unwrap();
        assert!(moved.is_file());
        assert!(!game_lnk.exists());
        assert!(browser_lnk.exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn dukeflaglogo_is_full_bleed_and_ico_has_all_required_layers() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let launcher_root = manifest_dir.parent().unwrap();
        let logo_path = launcher_root.join("src").join("assets").join("dukeflaglogo.png");
        let img = image::open(&logo_path)
            .expect("dukeflaglogo.png should load")
            .to_rgba8();
        let (w, h) = img.dimensions();
        assert_eq!(w, h, "logo must be square");
        assert!(w >= 256);

        let mut min_x = w;
        let mut min_y = h;
        let mut max_x = 0u32;
        let mut max_y = 0u32;
        for (x, y, px) in img.enumerate_pixels() {
            if px[3] > 8 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
        assert_eq!(min_x, 0, "left transparent border should be cropped edge-to-edge");
        assert_eq!(min_y, 0, "top transparent border should be cropped edge-to-edge");
        assert_eq!(max_x, w - 1, "right transparent border should be cropped edge-to-edge");
        assert_eq!(max_y, h - 1, "bottom transparent border should be cropped edge-to-edge");

        let ico_path = manifest_dir.join("icons").join("icon.ico");
        let ico = fs::read(&ico_path).expect("icon.ico should exist");
        assert!(ico.len() > 6 && u16::from_le_bytes([ico[0], ico[1]]) == 0 && u16::from_le_bytes([ico[2], ico[3]]) == 1);
        let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
        assert_eq!(count, 7);
        let mut dims: Vec<u32> = (0..count)
            .map(|i| {
                let off = 6 + i * 16;
                let w_b = ico[off];
                if w_b == 0 { 256 } else { w_b as u32 }
            })
            .collect();
        dims.sort_unstable();
        assert_eq!(dims, vec![16, 24, 32, 48, 64, 128, 256]);
    }

    #[test]
    fn launcher_settings_zoom_clamps_and_normalizes_unknown_themes() {
        let raw = r#"{
            "theme": "legacy-unknown-theme",
            "cardZoomPx": 999
        }"#;
        let (parsed, migrated) = normalize_loaded_settings(raw);
        assert!(migrated);
        assert_eq!(parsed.theme, "dark");
        assert_eq!(parsed.card_zoom_px, 280);
        assert!(parsed.projfs_streaming_enabled);
        assert!(parsed.shader_swarm_enabled);
        assert!(parsed.frame_pacing_enabled);

        let (low_zoom, low_mig) = normalize_loaded_settings(r#"{"cardZoomPx": 40}"#);
        assert!(low_mig);
        assert_eq!(low_zoom.card_zoom_px, 125);

        let def = LauncherSettings::default();
        assert_eq!(def.theme, "dark");
        assert_eq!(def.card_zoom_px, 185);
    }

    #[test]
    fn frame_pacing_and_ioring_diagnostics_report_valid_fields() {
        let ioring = duke_core::nvme_prefetch::probe_ioring_support();
        assert_eq!(ioring.min_required_build, 22621);
        assert!(!ioring.backend.is_empty());

        let fp = duke_core::win32_ffi::inspect_frame_pacing_status();
        assert!(!fp.hags_status.is_empty());
        assert!(fp.target_100ns_units > 0);
    }
}
