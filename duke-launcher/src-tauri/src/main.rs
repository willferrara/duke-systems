// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod achievements;
mod artwork;
mod bench;
mod bios_vault;
mod catalog_mmap;
mod deep_shrink;
mod delta_patch;
mod direct_storage;
mod duke_acoustics;
mod duke_ai_rt;
mod duke_comms;
mod duke_configurator;
mod duke_director;
mod duke_doctor;
mod duke_link;
mod duke_upscale;
mod duke_verified;
mod duke_vision;
mod emulators;
mod etw_xray;
mod friends;
mod importers;
mod latency_lab;
mod ludusavi;
mod mcp;
mod metadata_ext;
mod mod_dropper;
mod mod_graph;
mod ntfs_compress;
mod pe_inspect;
mod platforms;
mod projfs_stream;
mod quick_resume;
mod retro_achievements;
mod retro_catalog;
mod rom_inspect;
mod rom_patcher;
mod self_heal;
mod shader_swarm;
mod sidebar_icons;
mod sites;
mod snapshots;
mod sources;
mod steam_cm;
mod store_direct;
mod stream_extract;
mod swarm;
mod time_machine;
mod uninstaller;
mod couch_mode;
mod dkpack;
mod duke_capsule;
mod duke_ghost;
mod duke_jail;
mod duke_pilot;
mod duke_sandbox;
mod fix_finder;
mod privacy_vault;
mod vhdx_stage;

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

static DUKE_BUS: OnceLock<DukeBus> = OnceLock::new();
static DUKE_SHM_BUS: OnceLock<Mutex<Option<DukeShmBus>>> = OnceLock::new();

fn publish_shm_event_tagged(
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

fn publish_shm_event(event_type: ShmEventType, pid: u32, payload_a: u64, payload_b: u64) {
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
fn get_suite_autonomy_status() -> Value {
    let active = active_shm_apps();
    let shm_ready = DUKE_SHM_BUS
        .get()
        .and_then(|cell| cell.lock().ok().map(|g| g.is_some()))
        .unwrap_or(false);
    let pipe_online = DUKE_BUS.get().map(|b| b.is_online()).unwrap_or(false);

    let specs = [
        (
            "monitor",
            "Monitor",
            "GPU Power & EcoQoS",
            ShmSourceApp::DukeMonitor,
        ),
        (
            "capture",
            "Capture",
            "NVENC Replay & Clip Tagging",
            ShmSourceApp::DukeCapture,
        ),
        (
            "input",
            "Input",
            "5-Point Bezier & Virtual P2",
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
            "Standby"
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

    serde_json::json!({
        "shmReady": shm_ready,
        "pipeOnline": pipe_online,
        "connectedCount": connected_count,
        "totalCount": specs.len(),
        "daemons": daemons,
    })
}
static ACTIVE_SESSIONS: OnceLock<Mutex<HashMap<String, ActiveGameSession>>> = OnceLock::new();
static ACTIVE_JOBS: OnceLock<Mutex<HashMap<String, Arc<duke_core::win32_ffi::ProcessTreeJob>>>> =
    OnceLock::new();
static TRAY_DAEMON: OnceLock<duke_core::tray_daemon::TrayDaemonState> = OnceLock::new();
static HOTKEY_WATCHER_ACTIVE: AtomicBool = AtomicBool::new(false);

pub(crate) fn get_active_jobs_map(
) -> &'static Mutex<HashMap<String, Arc<duke_core::win32_ffi::ProcessTreeJob>>> {
    ACTIVE_JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}
static ACTIVE_SUSPEND_HOTKEY: OnceLock<Mutex<String>> = OnceLock::new();
static WEBVIEW_GRABBER_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
static PAUSE_FLAGS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();
static DOWNLOAD_QUEUE: OnceLock<Mutex<Vec<DownloadJob>>> = OnceLock::new();
static LIBRARY_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static QUEUE_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static SETTINGS_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static CATALOG_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static LIVE_CATALOG_UPSERTS: OnceLock<Mutex<HashMap<u32, CatalogGameEntry>>> = OnceLock::new();
static QUEUE_RUNNER_ACTIVE: AtomicBool = AtomicBool::new(false);
static PRICE_SYNC_ACTIVE: AtomicBool = AtomicBool::new(false);
static CATALOG_SYNC_ACTIVE: AtomicBool = AtomicBool::new(false);
static GLOBAL_SPEED_LIMIT_KBPS: AtomicU64 = AtomicU64::new(0);
static RAM_TRIM_MODE: AtomicU64 = AtomicU64::new(1);
static AUTO_FREEZE_MINIMIZED: AtomicBool = AtomicBool::new(false);
static AUTO_UNBLOCK_MOTW: AtomicBool = AtomicBool::new(true);

const CATALOG_TARGET_COUNT: usize = 30_000;
const CATALOG_HARVEST_PAGES: u32 = 30;
const CATALOG_SEMAPHORE_CONCURRENCY: usize = 4;

// 512 KB per-worker BufWriter capacity to cut ~25 MB of idle heap allocations during 8-stream downloads
const ASYNC_SEGMENT_BUFFER_BYTES: usize = 256 * 1024;
const REUSABLE_STREAM_CHUNK_BYTES: usize = 64 * 1024;
const SCAN_WINDOW_BYTES: usize = 64 * 1024;
const MAX_COVER_IMAGE_BYTES: usize = 256 * 1024;
const AIMD_MIN_CHUNK_BYTES: u64 = 4 * 1024 * 1024;
const AIMD_MAX_CHUNK_BYTES: u64 = 64 * 1024 * 1024;
const AIMD_STEP_BYTES: u64 = 4 * 1024 * 1024;
const WORK_STEAL_MIN_REMAINING_BYTES: u64 = 8 * 1024 * 1024;
const CATALOG_TTL_SECS: u64 = 24 * 60 * 60;
const HTTP_METADATA_TIMEOUT_SECS: u64 = 10;
pub(crate) const CREATE_NO_WINDOW: u32 = 0x08000000;

pub(crate) fn get_active_sessions() -> &'static Mutex<HashMap<String, ActiveGameSession>> {
    ACTIVE_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_active_hotkey_cell() -> &'static Mutex<String> {
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

#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
extern "system" {
    fn GetCompressedFileSizeW(
        lpFileName: *const u16,
        lpFileSizeHigh: *mut u32,
    ) -> u32;
}

pub(crate) use duke_core::win32_ffi::apply_backdrop_to_hwnd;
#[cfg(test)]
pub(crate) use duke_core::win32_ffi::{
    apply_backdrop_with, resolve_backdrop_type_for_theme, DwmMargins,
    DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMSBT_NONE, DWMSBT_TRANSIENTWINDOW,
};

#[tauri::command]
fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
fn apply_window_backdrop(app: AppHandle, theme_id: String) -> Result<(), String> {
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct GameEntry {
    pub id: String,
    pub title: String,
    pub exe_path: String,
    pub install_dir: String,
    pub size_bytes: u64,
    pub size_formatted: String,
    pub cover_url: String,
    pub hero_url: Option<String>,
    pub icon_path: Option<String>,
    pub genre: String,
    pub platform: String,
    pub source: String,
    #[serde(alias = "acquisition_source")]
    pub acquisition_source: String,
    pub is_favorite: bool,
    pub is_installed: bool,
    pub added_at: u64,
    pub last_played: Option<u64>,
    pub steam_app_id: Option<u64>,
    pub app_id: Option<u32>,
    pub playtime_seconds: u64,
    #[serde(alias = "imported_playtime_secs")]
    pub imported_playtime_secs: u64,
    #[serde(alias = "duke_playtime_secs")]
    pub duke_playtime_secs: u64,
    pub last_played_ts: Option<u64>,
    #[serde(alias = "steam_price_cents")]
    pub steam_price_cents: Option<u32>,
    #[serde(alias = "steam_price_formatted")]
    pub steam_price_formatted: Option<String>,
    #[serde(alias = "launch_args")]
    pub launch_args: Option<String>,
    #[serde(alias = "sandbox_enabled")]
    pub sandbox_enabled: bool,
    #[serde(alias = "nvme_prefetch_enabled")]
    pub nvme_prefetch_enabled: Option<bool>,
    #[serde(alias = "version_tag")]
    pub version_tag: String,
    #[serde(alias = "update_feed_url")]
    pub update_feed_url: String,
    #[serde(alias = "latest_version_tag")]
    pub latest_version_tag: String,
    #[serde(alias = "update_available")]
    pub update_available: bool,
    #[serde(alias = "achievements_path")]
    pub achievements_path: String,
    #[serde(default, alias = "platformId")]
    pub platform_id: u16,
    #[serde(default, alias = "consoleGroup")]
    pub console_group: u8,
    #[serde(default, alias = "romId")]
    pub rom_id: String,
    #[serde(default, alias = "emulatorId")]
    pub emulator_id: String,
}

impl GameEntry {
    pub fn total_playtime_secs(&self) -> u64 {
        let split = self
            .imported_playtime_secs
            .saturating_add(self.duke_playtime_secs);
        if split > 0 {
            split
        } else {
            self.playtime_seconds
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SteamNewsItem {
    pub title: String,
    pub date: u64,
    pub contents: String,
    pub feedlabel: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct GameSteamDetails {
    pub app_id: Option<u32>,
    pub title: String,
    pub short_description: String,
    pub developers: Vec<String>,
    pub publishers: Vec<String>,
    pub release_date: String,
    pub genres: Vec<String>,
    pub price_final: String,
    pub price_initial: Option<String>,
    pub discount_percent: u32,
    pub is_free: bool,
    pub steam_price_cents: Option<u32>,
    pub steam_price_formatted: Option<String>,
    pub live_players: Option<u64>,
    pub header_image: Option<String>,
    pub library_hero: Option<String>,
    pub hero_candidates: Vec<String>,
    pub news: Vec<SteamNewsItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SegmentChunk {
    pub index: usize,
    pub start_byte: u64,
    pub current_byte: u64,
    pub end_byte: u64,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DownloadJob {
    pub download_id: String,
    pub original_url: String,
    pub stream_url: String,
    #[serde(alias = "resolved_url")]
    pub resolved_url: String,
    pub referer: String,
    pub cookie: String,
    pub user_agent: String,
    pub file_name: String,
    pub save_dir: String,
    pub part_path: String,
    pub final_path: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub speed_mbps: f64,
    pub peak_speed_mbps: f64,
    pub progress_pct: f64,
    pub elapsed_secs: u64,
    pub eta_secs: Option<u64>,
    pub thread_count: usize,
    pub segments: Vec<SegmentChunk>,
    pub status: String,
    pub message: String,
    pub verdict: Option<String>,
    #[serde(alias = "registered_in_library")]
    pub registered_in_library: bool,
    pub added_at: u64,
}

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
}

fn default_disk_pressure_gb() -> f64 {
    50.0
}

fn default_disk_pressure_days() -> u32 {
    60
}

fn default_true() -> bool {
    true
}

fn default_theme_id() -> String {
    "dark".to_string()
}

fn default_card_zoom_px() -> u32 {
    185
}

fn normalize_theme_id(raw: &str) -> String {
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

fn is_false(v: &bool) -> bool {
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
        }
    }
}

fn effective_speed_limit_kbps(settings: &LauncherSettings) -> u64 {
    if settings.custom_speed_cap_mbps > 0.0 {
        return (settings.custom_speed_cap_mbps * 1024.0) as u64;
    }
    match settings.download_speed_preset.trim().to_ascii_lowercase().as_str() {
        "balanced" => 15 * 1024,
        "background" => 3 * 1024,
        _ => 0,
    }
}

fn apply_runtime_settings(settings: &LauncherSettings) {
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
    if let Ok(mut guard) = get_active_hotkey_cell().lock() {
        *guard = settings.suspend_hotkey.clone();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CatalogGameEntry {
    pub app_id: u32,
    pub name: String,
    pub price_cents: u32,
    #[serde(default, rename = "platformId")]
    pub platform_id: u16,
    #[serde(default, rename = "consoleGroup")]
    pub console_group: u8,
    #[serde(default, rename = "platformName")]
    pub platform_name: String,
    #[serde(default, rename = "platformSlug")]
    pub platform_slug: String,
    pub initial_price_cents: u32,
    pub discount_percent: u32,
    pub price_formatted: String,
    pub is_free: bool,
    pub ccu: u64,
    pub owners_min: u64,
    pub owners_max: u64,
    pub owners_label: String,
    pub positive: u64,
    pub negative: u64,
    pub review_pct: f64,
    pub is_singleplayer: bool,
    pub is_coop: bool,
    pub is_multiplayer: bool,
    pub is_online_only_pvp: bool,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub categories: Vec<String>,
    pub short_description: String,
    pub developers: Vec<String>,
    pub publishers: Vec<String>,
    pub release_date: String,
    pub cover_url: String,
    pub header_url: String,
    pub hero_url: String,
    pub value_score: f64,
    pub details_enriched: bool,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CatalogCacheFile {
    pub fetched_at: u64,
    pub fetched_pages: Vec<u32>,
    pub entries: Vec<CatalogGameEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CatalogResponse {
    pub games: Vec<CatalogGameEntry>,
    pub total_cached: usize,
    pub fetched_at: u64,
    pub from_cache: bool,
    pub stale_warning: Option<String>,
    pub live_search_matched: usize,
}

#[derive(Debug, Clone, Serialize)]
struct CatalogSyncProgressEvent {
    indexed_count: usize,
    target_count: usize,
    done: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEnrichResult {
    pub matched_count: usize,
    pub upserted_count: usize,
    pub total_indexed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SecurityScanReport {
    pub file_path: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub size_formatted: String,
    pub sha256: String,
    pub detected_format: String,
    pub verdict: String,
    pub summary: String,
    pub details: Vec<String>,
    pub shannon_entropy: f64,
    pub pe_architecture: Option<String>,
    pub pe_sections: Vec<String>,
    pub shield_badge: String,
    pub authenticode_status: String,
    pub authenticode_detail: String,
    pub imported_api_hits: Vec<String>,
    pub suspicious_behaviors: Vec<pe_inspect::PeSecurityFinding>,
    pub quarantine_path: Option<String>,
    pub zone_unblocked: bool,
}

#[derive(Debug, Clone)]
struct ResolvedStreamInfo {
    stream_url: String,
    referer: String,
    cookie: String,
    user_agent: String,
    suggested_filename: Option<String>,
}

fn get_pause_map() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    PAUSE_FLAGS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_queue_store() -> &'static Mutex<Vec<DownloadJob>> {
    DOWNLOAD_QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

fn get_webview_grabber_lock() -> &'static tokio::sync::Mutex<()> {
    WEBVIEW_GRABBER_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn rotate_privacy_user_agent() -> &'static str {
    duke_core::net_doh::rotate_privacy_user_agent()
}

fn get_session_user_agent() -> &'static str {
    duke_core::net_doh::get_session_user_agent()
}

pub(crate) fn validate_external_http_url(raw_url: &str) -> Result<Url, String> {
    let trimmed = raw_url.trim();
    if !trimmed.starts_with("https://") && !trimmed.starts_with("http://") {
        return Err("Security Error: Only http:// and https:// URLs are permitted.".to_string());
    }
    let parsed = Url::parse(trimmed).map_err(|e| format!("Invalid URL syntax: {}", e))?;
    match parsed.scheme() {
        "http" | "https" => {}
        _ => return Err("Security Error: Unsupported URL scheme.".to_string()),
    }
    if parsed.host_str().unwrap_or("").is_empty() {
        return Err("Security Error: URL is missing a valid hostname.".to_string());
    }
    Ok(parsed)
}

pub(crate) fn resolve_default_downloads_dir(app: &AppHandle) -> PathBuf {
    let settings = load_settings_from_disk(app);
    if let Some(custom) = settings.default_save_dir.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return PathBuf::from(custom);
    }
    if let Ok(dl) = app.path().download_dir() {
        return dl;
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let candidate = PathBuf::from(profile).join("Downloads");
        if candidate.exists() {
            return candidate;
        }
    }
    std::env::temp_dir().join("DukeLauncherDownloads")
}

pub(crate) fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

pub(crate) fn resolve_game_root_dir(exe_or_dir: &Path) -> PathBuf {
    let base_dir = if exe_or_dir.is_file() {
        exe_or_dir
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| exe_or_dir.to_path_buf())
    } else {
        exe_or_dir.to_path_buf()
    };

    let generic_folders = [
        "bin", "binaries", "win64", "win32", "x64", "x86", "shipping", "release", "game", "retail",
        "engine", "build",
    ];

    let mut current = base_dir;
    for _ in 0..4 {
        if let Some(folder_name) = current.file_name().and_then(|n| n.to_str()) {
            let lower = folder_name.to_lowercase();
            if generic_folders.contains(&lower.as_str()) {
                if let Some(parent) = current.parent() {
                    if parent.parent().is_some() {
                        current = parent.to_path_buf();
                        continue;
                    }
                }
            }
        }
        break;
    }

    current
}

pub(crate) fn clean_game_title(raw: &str) -> String {
    let lower_raw = raw.trim().to_lowercase();
    if lower_raw == "gzpro" || lower_raw.contains("gzpro") {
        return "Godzilla: Destroy All Monsters Melee".to_string();
    }

    let mut without_ext = raw.trim();
    for ext in [
        ".part1.rar",
        ".part01.rar",
        ".part001.rar",
        ".7z.001",
        ".zip.001",
        ".tar.gz",
        ".tgz",
        ".rar",
        ".7z",
        ".zip",
        ".iso",
        ".tar",
        ".gz",
        ".exe",
    ] {
        if without_ext.to_lowercase().ends_with(ext) {
            without_ext = &without_ext[..without_ext.len().saturating_sub(ext.len())];
            break;
        }
    }

    let mut spaced = String::with_capacity(without_ext.len() + 8);
    let chars: Vec<char> = without_ext.chars().collect();
    for i in 0..chars.len() {
        let c = chars[i];
        if i > 0 {
            let prev = chars[i - 1];
            let next_is_lower = i + 1 < chars.len() && chars[i + 1].is_lowercase();
            if (prev.is_lowercase() && c.is_uppercase())
                || (prev.is_uppercase() && c.is_uppercase() && next_is_lower)
            {
                spaced.push(' ');
            }
        }
        spaced.push(c);
    }

    let junk_tokens = [
        "-Win64-Shipping",
        "-Win32-Shipping",
        "Win64",
        "Win32",
        "x64",
        "-Portable",
        "Portable",
        "Repack",
        "Launcher",
    ];

    let mut cleaned = spaced.replace(['_', '.'], " ");
    for token in junk_tokens {
        cleaned = cleaned.replace(token, "");
    }

    let final_str = cleaned
        .trim_matches(|c: char| c == '-' || c == ' ')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if final_str.is_empty() {
        "Unknown Game".to_string()
    } else {
        final_str
    }
}

/// Unconditionally strips `:Zone.Identifier` NTFS streams on `.exe`, `.dll`, `.asi`, `.bat` recursively.
fn unblock_ntfs_zone_identifier_raw(target: &Path) -> usize {
    self_heal::unblock_game_tree_zone_identifiers(target)
}

/// Automatically strips the Windows NTFS Mark-of-the-Web (`:Zone.Identifier` alternate data stream)
/// when `auto_unblock_motw` is enabled so Windows SmartScreen never blocks or slows down first game launch.
fn unblock_ntfs_zone_identifier(target: &Path) {
    if !AUTO_UNBLOCK_MOTW.load(Ordering::Relaxed) {
        return;
    }
    let _ = unblock_ntfs_zone_identifier_raw(target);
}

/// Validates that no path inside `dest_dir` contains `..` traversal, external symlinks, or decompression bombs (Zip-Slip protection),
/// and automatically unblocks NTFS `:Zone.Identifier` streams on all extracted files.
fn validate_extracted_tree_against_zip_slip(dest_dir: &Path) -> Result<(), String> {
    let canonical_root = dest_dir
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize extraction directory: {}", e))?;

    let mut file_count: usize = 0;
    const MAX_EXTRACTED_FILES: usize = 250_000;

    for entry in WalkDir::new(dest_dir).follow_links(false).into_iter().flatten() {
        let p = entry.path();
        for comp in p.components() {
            if matches!(comp, Component::ParentDir) {
                let _ = fs::remove_file(p);
                return Err("Security Error: Archive contained path traversal ('..') entry.".to_string());
            }
        }

        if entry.path_is_symlink() {
            if let Ok(target_link) = fs::read_link(p) {
                if target_link.is_absolute()
                    || target_link
                        .components()
                        .any(|c| matches!(c, Component::ParentDir))
                {
                    let _ = fs::remove_file(p);
                    return Err(
                        "Security Error: Blocked external symlink inside extracted archive."
                            .to_string(),
                    );
                }
            }
        }

        if let Ok(canon_child) = p.canonicalize() {
            if !canon_child.starts_with(&canonical_root) {
                let _ = fs::remove_file(p);
                return Err(
                    "Security Error: Zip-Slip path traversal detected outside destination folder."
                        .to_string(),
                );
            }
        }

        if entry.file_type().is_file() {
            file_count += 1;
            if file_count > MAX_EXTRACTED_FILES {
                return Err(
                    "Security Error: Decompression bomb protection triggered (excessive file count)."
                        .to_string(),
                );
            }
        }
    }

    let q_dir = if cfg!(test) {
        None
    } else {
        pe_inspect::quarantine_root_dir_standalone().ok()
    };
    let shield_summary = pe_inspect::gate_game_directory_pe_security(dest_dir, q_dir.as_deref(), 256);
    if let Some((blocked_path, reason, q_path)) = shield_summary.blocked_files.first() {
        let name = blocked_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("binary");
        let q_note = q_path
            .as_ref()
            .map(|p| format!(" (quarantined to {})", p.display()))
            .unwrap_or_default();
        return Err(format!(
            "Security Shield v2 blocked high-risk binary '{}' inside archive{}: {}",
            name, q_note, reason
        ));
    }

    unblock_ntfs_zone_identifier(dest_dir);
    Ok(())
}

fn is_supported_archive_name(lower_name: &str) -> bool {
    lower_name.ends_with(".rar")
        || lower_name.ends_with(".7z")
        || lower_name.ends_with(".zip")
        || lower_name.ends_with(".tar")
        || lower_name.ends_with(".gz")
        || lower_name.ends_with(".tgz")
        || lower_name.ends_with(".tar.zst")
        || lower_name.ends_with(".tzst")
        || lower_name.ends_with(".zst")
        || lower_name.ends_with(".iso")
        || lower_name.ends_with(".7z.001")
        || lower_name.ends_with(".zip.001")
        || lower_name.ends_with(".part1.rar")
        || lower_name.ends_with(".part01.rar")
}

/// Extracts `.rar`, `.7z`, `.zip`, `.tar`, `.gz`, and multi-part archives (`.part1.rar`, `.7z.001`) into `dest_dir`
/// while enforcing strict Zip-Slip / Path Traversal protection.
pub(crate) fn extract_archive_to_folder_sync(archive_path: &Path, dest_dir: &Path) -> Result<(), String> {
    for comp in archive_path.components() {
        if matches!(comp, Component::ParentDir) {
            return Err("Security Error: Invalid archive path traversal.".to_string());
        }
    }

    if !dest_dir.exists() {
        fs::create_dir_all(dest_dir)
            .map_err(|e| format!("Failed to create extraction folder: {}", e))?;
    }

    let archive_str = archive_path.to_string_lossy().to_string();
    let dest_str = dest_dir.to_string_lossy().to_string();
    let dest_with_slash = format!("{}\\", dest_str.trim_end_matches('\\'));

    let prog_files = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string());
    let prog_files_x86 =
        std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".to_string());
    let local_appdata = std::env::var("LOCALAPPDATA").unwrap_or_default();

    // 1. Check 7-Zip (`C:\Program Files\7-Zip\7z.exe`, x86, LocalAppData, and PATH `7z`)
    // 7-Zip natively supports .7z, .7z.001, .rar, .part1.rar, .zip, .tar, .gz, .iso
    let mut seven_zip_candidates = vec![
        PathBuf::from(&prog_files).join("7-Zip").join("7z.exe"),
        PathBuf::from(&prog_files_x86).join("7-Zip").join("7z.exe"),
    ];
    if !local_appdata.is_empty() {
        seven_zip_candidates.push(PathBuf::from(&local_appdata).join("Programs\\7-Zip\\7z.exe"));
    }

    for sz_bin in &seven_zip_candidates {
        if sz_bin.exists() {
            let out_arg = format!("-o{}", dest_str);
            let mut cmd = Command::new(sz_bin);
            cmd.creation_flags(CREATE_NO_WINDOW);
            cmd.args(["x", "-y", "-spf-", &out_arg, &archive_str]);
            if let Ok(status) = cmd.status() {
                if status.success() {
                    validate_extracted_tree_against_zip_slip(dest_dir)?;
                    return Ok(());
                }
            }
        }
    }

    // Try `7z` on system PATH
    {
        let out_arg = format!("-o{}", dest_str);
        let mut cmd = Command::new("7z");
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd.args(["x", "-y", "-spf-", &out_arg, &archive_str]);
        if let Ok(status) = cmd.status() {
            if status.success() {
                validate_extracted_tree_against_zip_slip(dest_dir)?;
                return Ok(());
            }
        }
    }

    // 2. Check WinRAR / UnRAR (`UnRAR.exe`, `WinRAR.exe`)
    let mut winrar_candidates = vec![
        PathBuf::from(&prog_files).join("WinRAR").join("UnRAR.exe"),
        PathBuf::from(&prog_files).join("WinRAR").join("WinRAR.exe"),
        PathBuf::from(&prog_files_x86).join("WinRAR").join("UnRAR.exe"),
        PathBuf::from(&prog_files_x86).join("WinRAR").join("WinRAR.exe"),
    ];
    if !local_appdata.is_empty() {
        winrar_candidates.push(PathBuf::from(&local_appdata).join("Programs\\WinRAR\\UnRAR.exe"));
        winrar_candidates.push(PathBuf::from(&local_appdata).join("Programs\\WinRAR\\WinRAR.exe"));
    }

    for rar_bin in &winrar_candidates {
        if rar_bin.exists() {
            let mut cmd = Command::new(rar_bin);
            cmd.creation_flags(CREATE_NO_WINDOW);
            let bin_str = rar_bin.to_string_lossy();
            if bin_str.ends_with("WinRAR.exe") {
                cmd.args(["x", "-ibck", "-y", &archive_str, &dest_with_slash]);
            } else {
                cmd.args(["x", "-y", &archive_str, &dest_with_slash]);
            }
            if let Ok(status) = cmd.status() {
                if status.success() {
                    validate_extracted_tree_against_zip_slip(dest_dir)?;
                    return Ok(());
                }
            }
        }
    }

    // 3. Try Windows 11 native bsdtar (`tar.exe` supports .rar, .7z, .zip, .tar, .gz)
    // First inspect archive listing via `tar.exe -tf` to block any `..` traversal before unpacking
    let mut tar_list = Command::new("tar.exe");
    tar_list.creation_flags(CREATE_NO_WINDOW);
    tar_list.args(["-tf", &archive_str]);
    if let Ok(out) = tar_list.output() {
        if out.status.success() {
            let listing = String::from_utf8_lossy(&out.stdout);
            for line in listing.lines() {
                if line.contains("..\\") || line.contains("../") || line.starts_with('/') {
                    return Err(
                        "Security Error: Blocked malicious Zip-Slip path inside archive.".to_string(),
                    );
                }
            }
        }
    }

    let mut tar_cmd = Command::new("tar.exe");
    tar_cmd.creation_flags(CREATE_NO_WINDOW);
    tar_cmd.args(["-xf", &archive_str, "-C", &dest_str]);
    if let Ok(status) = tar_cmd.status() {
        if status.success() {
            validate_extracted_tree_against_zip_slip(dest_dir)?;
            return Ok(());
        }
    }

    // 4. Fallback to PowerShell Expand-Archive for .zip files
    if archive_str.to_lowercase().ends_with(".zip") {
        let ps_cmd = format!(
            "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
            archive_str.replace('\'', "''"),
            dest_str.replace('\'', "''")
        );
        let mut cmd = Command::new("powershell");
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", &ps_cmd]);
        if let Ok(status) = cmd.status() {
            if status.success() {
                validate_extracted_tree_against_zip_slip(dest_dir)?;
                return Ok(());
            }
        }
    }

    Err("Could not extract archive automatically. Make sure 7-Zip, WinRAR, or Windows 11 Archive support is installed.".to_string())
}

const IGNORED_GAME_SUBDIRS: &[&str] = &[
    "_commonredist",
    "redist",
    "directx",
    "vcredist",
    "dotnet",
    "easyanticheat",
    "battleye",
    "support",
    "installers",
    "mono",
    "crashreporter",
];

const PENALIZED_HELPER_EXES: &[&str] = &[
    "unitycrashhandler32",
    "unitycrashhandler64",
    "unitycrashhandler",
    "unrealcefsubprocess",
    "ueprereqsetup_x64",
    "ueprereqsetup",
    "ue4prereqsetup",
    "crashreportclient",
    "crashreport",
    "crashhandler",
    "vc_redist",
    "vcredist",
    "dxsetup",
    "dxwebsetup",
    "dotnetfx",
    "dotnet",
    "unins000",
    "unins0",
    "unins",
    "uninstall",
    "setup",
    "easyanticheat",
    "battleye",
    "steamclient_loader",
    "benchmark",
    "dedicatedserver",
    "crs-handler",
];

/// Scores a candidate `.exe` inside a game folder. Helper utilities, crash handlers,
/// uninstallers, and redistributables receive a heavy negative penalty so they never beat
/// a genuine game executable.
pub(crate) fn score_exe_candidate(
    path: &Path,
    file_size: u64,
    expected_title: &str,
    title_words: &[String],
) -> i64 {
    let path_lower = path.to_string_lossy().to_lowercase();
    if IGNORED_GAME_SUBDIRS.iter().any(|d| path_lower.contains(d)) {
        return i64::MIN / 2;
    }

    let file_name_lower = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let stem_lower = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    if PENALIZED_HELPER_EXES
        .iter()
        .any(|bad| stem_lower.contains(bad) || file_name_lower.starts_with(bad))
    {
        return i64::MIN / 2 + (file_size.min(100_000) as i64);
    }

    let mut score: i64 = i64::try_from(file_size).unwrap_or(i64::MAX / 4);
    let clean_expected_slug: String = clean_game_title(expected_title)
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect();
    let stem_slug: String = stem_lower
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect();

    if !clean_expected_slug.is_empty() && stem_slug == clean_expected_slug {
        score = score.saturating_add(800 * 1024 * 1024);
    } else if title_words.iter().any(|w| stem_lower.contains(w)) {
        score = score.saturating_add(500 * 1024 * 1024);
    }

    if file_name_lower.ends_with("-win64-shipping.exe")
        || file_name_lower.ends_with("-win32-shipping.exe")
        || stem_lower.contains("shipping")
    {
        score = score.saturating_add(450 * 1024 * 1024);
    }

    if let Some(parent) = path.parent() {
        if parent.join("UnityPlayer.dll").exists()
            || parent.join(format!("{}_Data", stem_lower)).exists()
        {
            score = score.saturating_add(400 * 1024 * 1024);
        }
        if parent.join(format!("{}.pck", stem_lower)).exists()
            || parent.join("data.pck").exists()
        {
            score = score.saturating_add(380 * 1024 * 1024);
        }
    }

    score
}

/// Scans up to `max_depth` directory levels deep to pick the highest-scoring playable `.exe`.
pub(crate) fn find_best_game_exe_with_max_depth(
    folder: &Path,
    expected_game_title: &str,
    max_depth: usize,
) -> Option<PathBuf> {
    if !folder.exists() {
        return None;
    }

    let title_words: Vec<String> = clean_game_title(expected_game_title)
        .to_lowercase()
        .split_whitespace()
        .filter(|w| w.len() >= 3)
        .map(|w| w.to_string())
        .collect();

    let mut best_candidate: Option<(PathBuf, i64)> = None;

    for entry in WalkDir::new(folder)
        .max_depth(max_depth.max(1))
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if ext != "exe" {
            continue;
        }

        let file_size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if file_size < 150 * 1024 {
            continue;
        }

        let score = score_exe_candidate(path, file_size, expected_game_title, &title_words);
        if score < 0 {
            continue;
        }

        match &best_candidate {
            Some((_, best_score)) if score <= *best_score => {}
            _ => {
                best_candidate = Some((path.to_path_buf(), score));
            }
        }
    }

    best_candidate.map(|(p, _)| p)
}

/// Scans up to 6 levels deep to find the best playable `.exe`, scoring by file size, title match,
/// Unreal (`-Win64-Shipping.exe`), Unity (`UnityPlayer.dll` sibling), and Godot (`.pck` sibling) bonuses,
/// while strictly blacklisting crash handlers, CEF subprocesses, installers, and redistributables.
pub(crate) fn find_best_game_exe_in_folder(folder: &Path, expected_game_title: &str) -> Option<PathBuf> {
    find_best_game_exe_with_max_depth(folder, expected_game_title, 6)
}

fn is_rom_or_console_image_name(lower_name: &str) -> bool {
    const ROM_EXTS: &[&str] = &[
        ".rvz", ".chd", ".3ds", ".cci", ".nsp", ".xci", ".gba", ".nds", ".z64", ".n64", ".v64",
        ".sfc", ".smc", ".nes", ".gb", ".gbc", ".wbfs", ".gcm", ".cso",
    ];
    ROM_EXTS.iter().any(|ext| lower_name.ends_with(ext))
}

async fn install_detected_rom_to_library(
    app: &AppHandle,
    rom_path: &Path,
    clean_title: &str,
) -> Result<GameEntry, String> {
    let rp = rom_path.to_path_buf();
    let inspection = tokio::task::spawn_blocking(move || rom_inspect::inspect_rom_file(&rp))
        .await
        .map_err(|e| format!("ROM inspection panicked: {e}"))?;
    let plat_id = inspection
        .as_ref()
        .map(|i| i.platform_id)
        .filter(|&id| id > 0)
        .unwrap_or_else(|| {
            let (inferred, _) =
                artwork::infer_game_platform_hint(clean_title, &rom_path.to_string_lossy(), "", "");
            inferred.unwrap_or(platforms::PLATFORM_N64)
        });
    let fallback_title = clean_title.trim().to_string();
    let mut entry = {
        let mut library = load_library_from_disk(app)?;
        let out = upsert_retro_game_entry_in_library(
            &mut library,
            RetroUpsertParams {
                rom_path: Some(rom_path),
                inspection: inspection.as_ref(),
                platform_id: plat_id,
                custom_title: Some(&fallback_title),
                catalog_app_id: None,
                cover_hint: None,
                hero_hint: None,
                origin_source: "turbo_downloader",
            },
        );
        save_library_to_disk(app, &library)?;
        out
    };

    if let (Ok(covers_dir), Ok(icons_dir)) = (
        get_covers_dir(app),
        sidebar_icons::get_sidebar_icons_dir(app),
    ) {
        let settings = load_settings_from_disk(app);
        let (_, plat_hint) =
            artwork::infer_game_platform_hint(&entry.title, &entry.exe_path, &entry.genre, &entry.source);
        if let Some(outcome) = artwork::resolve_and_cache_cover_in_dirs(
            &artwork::CoverResolveRequest {
                cache_id: entry.id.clone(),
                title: entry.title.clone(),
                platform_id: (entry.platform_id > 0).then_some(entry.platform_id),
                platform_hint: plat_hint,
                steam_app_id: None,
                existing_cover_url: (!entry.cover_url.is_empty()).then(|| entry.cover_url.clone()),
                steamgriddb_api_key: settings.steamgriddb_api_key.unwrap_or_default(),
                sidebar_icon_id: Some(entry.id.clone()),
            },
            &covers_dir,
            &icons_dir,
        )
        .await
        {
            entry.cover_url = outcome.cover_path.to_string_lossy().to_string();
            if let Ok(mut lib) = load_library_from_disk(app) {
                if let Some(found) = lib.iter_mut().find(|g| g.id == entry.id) {
                    found.cover_url = entry.cover_url.clone();
                }
                let _ = save_library_to_disk(app, &lib);
            }
        }
    }

    if let Ok(lib) = load_library_from_disk(app) {
        let _ = app.emit("library-updated", &lib);
    }
    Ok(entry)
}

async fn auto_install_downloaded_file(
    app: &AppHandle,
    downloaded_file_path: &str,
) -> Result<GameEntry, String> {
    let path = PathBuf::from(downloaded_file_path);
    if !path.exists() {
        return Err(format!("Downloaded file not found at: {}", downloaded_file_path));
    }

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Game")
        .to_string();
    let lower_name = file_name.to_lowercase();
    let clean_title = clean_game_title(&file_name);

    // Case 1: Downloaded file is already a direct .exe
    if lower_name.ends_with(".exe") {
        let added = add_local_game_with_origin(
            app.clone(),
            downloaded_file_path.to_string(),
            Some(clean_title),
            None,
            Some("Ready".to_string()),
            None,
            Some("turbo_downloader"),
        )
        .await?;
        if let Ok(lib) = load_library_from_disk(app) {
            let _ = app.emit("library-updated", &lib);
        }
        return Ok(added);
    }

    // Case 1b: Downloaded file is a direct console ROM / disc image (.rvz, .chd, .3ds, .cci, .nsp, .xci, .gba, .nds, .z64, .sfc, .nes, or console .iso)
    if is_rom_or_console_image_name(&lower_name) {
        return install_detected_rom_to_library(app, &path, &clean_title).await;
    }
    if lower_name.ends_with(".iso") {
        let p_check = path.clone();
        let is_console_iso = tokio::task::spawn_blocking(move || rom_inspect::inspect_rom_file(&p_check))
            .await
            .ok()
            .flatten()
            .is_some_and(|insp| insp.platform_id != platforms::PLATFORM_PC && insp.platform_id > 0);
        if is_console_iso {
            return install_detected_rom_to_library(app, &path, &clean_title).await;
        }
    }

    // Case 2: Downloaded file is a supported archive (.rar, .7z, .zip, .tar, .tar.zst, .iso, etc.)
    if !is_supported_archive_name(&lower_name) {
        return Err("File is not a recognized game archive, ROM, or executable.".to_string());
    }

    let parent_dir = path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| resolve_default_downloads_dir(app));

    let folder_safe_name = sanitize_filename(&clean_title);
    let extract_dir = parent_dir.join(&folder_safe_name);

    let target_exe = match find_best_game_exe_in_folder(&extract_dir, &clean_title) {
        Some(existing_exe) => Some(existing_exe),
        None => {
            let arch_clone = path.clone();
            let dest_clone = extract_dir.clone();
            tokio::task::spawn_blocking(move || {
                extract_archive_to_folder_sync(&arch_clone, &dest_clone)
            })
            .await
            .map_err(|e| format!("Extraction task panicked: {}", e))??;

            trim_process_working_set();
            find_best_game_exe_in_folder(&extract_dir, &clean_title)
        }
    };

    if let Some(exe_pb) = target_exe {
        let exe_str = exe_pb.to_string_lossy().to_string();
        let added = add_local_game_with_origin(
            app.clone(),
            exe_str,
            Some(clean_title),
            None,
            Some("Ready".to_string()),
            None,
            Some("turbo_downloader"),
        )
        .await?;

        if let Ok(lib) = load_library_from_disk(app) {
            let _ = app.emit("library-updated", &lib);
        }

        trim_process_working_set();
        return Ok(added);
    }

    // Fallback for extracted console ROMs inside .zip / .7z / .tar.zst archives (e.g. Vimm's Lair)
    let dir_scan = extract_dir.clone();
    let roms = tokio::task::spawn_blocking(move || rom_inspect::scan_folder_for_roms(&dir_scan, 4))
        .await
        .map_err(|e| format!("ROM scan panicked: {e}"))?;
    if let Some(first_rom) = roms.first() {
        let rom_entry =
            install_detected_rom_to_library(app, Path::new(&first_rom.path), &clean_title).await?;
        trim_process_working_set();
        return Ok(rom_entry);
    }

    Err(format!(
        "Extracted to '{}', but no game .exe or console ROM was found inside.",
        extract_dir.display()
    ))
}

/// Queries Windows NTFS physical allocated file size (`GetCompressedFileSizeW`) so NTFS LZX/XPRESS8K
/// compression savings from `compact.exe` are accurately reflected in disk usage telemetry.
fn get_physical_file_size_on_disk(path: &Path) -> u64 {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut high: u32 = 0;
        let low = unsafe { GetCompressedFileSizeW(wide.as_ptr(), &mut high) };
        if low != 0xFFFF_FFFF {
            return ((high as u64) << 32) | (low as u64);
        }
    }
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

pub(crate) fn calculate_dir_size_sync(target_path: &Path) -> u64 {
    if target_path.is_file() {
        return get_physical_file_size_on_disk(target_path);
    }

    WalkDir::new(target_path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            entry.path() == target_path
                || !matches!(
                    entry.file_name().to_str(),
                    Some(".duke-cas" | ".duke-profile" | ".duke-patch-tmp")
                )
        })
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| get_physical_file_size_on_disk(entry.path()))
        .sum()
}

/// Checks if Portable Mode is active (`portable.txt`, `portable.flag`, or `data/` directory alongside the executable).
/// When active, all state (`library.json`, `settings.json`, `catalog.dkcat`, `emulators/`, `.duke-cas/`) lives relative
/// to the executable with zero host `AppData` footprint.
fn resolve_portable_data_dir() -> Option<PathBuf> {
    let exe_path = std::env::current_exe().ok()?;
    let exe_dir = exe_path.parent()?;
    let txt_file = exe_dir.join("portable.txt");
    let flag_file = exe_dir.join("portable.flag");
    let data_dir = exe_dir.join("data");
    if txt_file.exists() || flag_file.exists() || data_dir.is_dir() {
        if !data_dir.exists() {
            let _ = fs::create_dir_all(&data_dir);
        }
        return Some(data_dir);
    }
    None
}

pub(crate) fn resolve_data_root_dir_standalone() -> Result<PathBuf, String> {
    if let Some(portable_dir) = resolve_portable_data_dir() {
        return Ok(portable_dir);
    }

    let base_appdata = std::env::var("APPDATA")
        .or_else(|_| std::env::var("LOCALAPPDATA"))
        .map(|p| PathBuf::from(p).join("com.dukelauncher.desktop"))
        .map_err(|e| format!("Failed to resolve AppData directory: {}", e))?;

    if !base_appdata.exists() {
        fs::create_dir_all(&base_appdata)
            .map_err(|e| format!("Failed to create AppData directory: {}", e))?;
    }
    Ok(base_appdata)
}

pub(crate) fn get_app_data_file_standalone(filename: &str) -> Result<PathBuf, String> {
    let data_root_dir = resolve_data_root_dir_standalone()?;
    let target_file = data_root_dir.join(filename);
    if !target_file.exists() && resolve_portable_data_dir().is_none() {
        if let Some(parent) = data_root_dir.parent() {
            if let Ok(entries) = fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let dir_name = entry.file_name().to_string_lossy().to_lowercase();
                    if dir_name.contains("dukelauncher") && entry.path() != data_root_dir {
                        let legacy_candidate = entry.path().join(filename);
                        if legacy_candidate.exists() {
                            let _ = fs::copy(&legacy_candidate, &target_file);
                            break;
                        }
                    }
                }
            }
        }
    }
    if self_heal::is_protected_json_file(&target_file) {
        let _ = self_heal::recover_protected_json_if_corrupt(&target_file);
    }
    Ok(target_file)
}

pub(crate) fn get_covers_dir_standalone() -> Result<PathBuf, String> {
    let root = resolve_data_root_dir_standalone()?;
    let covers_dir = root.join("covers");
    if !covers_dir.exists() {
        fs::create_dir_all(&covers_dir)
            .map_err(|e| format!("Failed to create covers directory: {}", e))?;
    }
    Ok(covers_dir)
}

pub(crate) fn get_backups_dir_standalone() -> Result<PathBuf, String> {
    if let Some(portable_dir) = resolve_portable_data_dir() {
        let backups = portable_dir.join("backups").join("Duke_Save_Backups");
        if !backups.exists() {
            fs::create_dir_all(&backups)
                .map_err(|e| format!("Failed to create portable backups directory: {}", e))?;
        }
        return Ok(backups);
    }

    let downloads_root = std::env::var("USERPROFILE")
        .map(|p| PathBuf::from(p).join("Downloads"))
        .ok()
        .filter(|p| p.exists())
        .unwrap_or_else(|| std::env::temp_dir().join("DukeLauncherDownloads"));
    let backups_dir = downloads_root.join("Duke_Save_Backups");
    if !backups_dir.exists() {
        fs::create_dir_all(&backups_dir)
            .map_err(|e| format!("Failed to create Duke_Save_Backups directory: {}", e))?;
    }
    Ok(backups_dir)
}

pub(crate) fn resolve_data_root_dir(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(portable_dir) = resolve_portable_data_dir() {
        return Ok(portable_dir);
    }

    let app_data_dir = app
        .path()
        .app_data_dir()
        .or_else(|_| {
            std::env::var("APPDATA")
                .map(|p| PathBuf::from(p).join("com.dukelauncher.desktop"))
        })
        .map_err(|e| format!("Failed to resolve AppData directory: {}", e))?;

    if !app_data_dir.exists() {
        fs::create_dir_all(&app_data_dir)
            .map_err(|e| format!("Failed to create AppData directory: {}", e))?;
    }

    Ok(app_data_dir)
}

pub(crate) fn get_covers_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let root = resolve_data_root_dir(app)?;
    let covers_dir = root.join("covers");
    if !covers_dir.exists() {
        fs::create_dir_all(&covers_dir)
            .map_err(|e| format!("Failed to create covers directory: {}", e))?;
    }
    Ok(covers_dir)
}

fn get_backups_dir(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(portable_dir) = resolve_portable_data_dir() {
        let backups = portable_dir.join("backups").join("Duke_Save_Backups");
        if !backups.exists() {
            fs::create_dir_all(&backups)
                .map_err(|e| format!("Failed to create portable backups directory: {}", e))?;
        }
        return Ok(backups);
    }

    let downloads_root = resolve_default_downloads_dir(app);
    let backups_dir = downloads_root.join("Duke_Save_Backups");
    if !backups_dir.exists() {
        fs::create_dir_all(&backups_dir)
            .map_err(|e| format!("Failed to create Duke_Save_Backups directory: {}", e))?;
    }
    Ok(backups_dir)
}

fn get_app_data_file(app: &AppHandle, filename: &str) -> Result<PathBuf, String> {
    let data_root_dir = resolve_data_root_dir(app)?;
    let target_file = data_root_dir.join(filename);

    // Seamless migration if user has an existing library.json / downloads.json in an older *dukelauncher* folder
    if !target_file.exists() && resolve_portable_data_dir().is_none() {
        if let Some(parent) = data_root_dir.parent() {
            if let Ok(entries) = fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let dir_name = entry.file_name().to_string_lossy().to_lowercase();
                    if dir_name.contains("dukelauncher") && entry.path() != data_root_dir {
                        let legacy_candidate = entry.path().join(filename);
                        if legacy_candidate.exists() {
                            let _ = fs::copy(&legacy_candidate, &target_file);
                            break;
                        }
                    }
                }
            }
        }
    }

    if self_heal::is_protected_json_file(&target_file) {
        let _ = self_heal::recover_protected_json_if_corrupt(&target_file);
    }

    Ok(target_file)
}

/// Persists cover art into the dedicated `covers/` folder (`covers/<safe_id>.cache` + binary image file)
/// so `library.json` remains tiny and zero-bloat on disk.
fn persist_cover_to_disk_cache(app: &AppHandle, game_id: &str, cover_str: &str) -> String {
    let trimmed = cover_str.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if !trimmed.starts_with("data:image/") {
        if trimmed.len() <= 2048 {
            return trimmed.to_string();
        }
        return String::new();
    }

    let max_b64_chars = (MAX_COVER_IMAGE_BYTES * 4) / 3 + 128;
    if trimmed.len() > max_b64_chars {
        return String::new();
    }

    let safe_id: String = game_id
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .take(80)
        .collect();
    if safe_id.is_empty() {
        return trimmed.to_string();
    }

    if let Ok(covers_dir) = get_covers_dir(app) {
        let cache_file = covers_dir.join(format!("{}.cover", safe_id));
        if atomic_write_string(&cache_file, trimmed).is_ok() {
            return format!("cover-cache://{}", safe_id);
        }
    }
    trimmed.to_string()
}

fn hydrate_cover_from_disk_cache(app: &AppHandle, game_id: &str, stored_cover: &str) -> String {
    let trimmed = stored_cover.trim();
    if let Some(cache_key) = trimmed.strip_prefix("cover-cache://") {
        if let Ok(covers_dir) = get_covers_dir(app) {
            let cache_file = covers_dir.join(format!("{}.cover", cache_key.trim()));
            if let Ok(data_uri) = fs::read_to_string(&cache_file) {
                if !data_uri.trim().is_empty() {
                    return data_uri;
                }
            }
        }
    }
    let safe_id: String = game_id
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .take(80)
        .collect();
    if !safe_id.is_empty() {
        if let Ok(covers_dir) = get_covers_dir(app) {
            let cache_file = covers_dir.join(format!("{}.cover", safe_id));
            if trimmed.is_empty() && cache_file.exists() {
                if let Ok(data_uri) = fs::read_to_string(&cache_file) {
                    if !data_uri.trim().is_empty() {
                        return data_uri;
                    }
                }
            }
        }
    }
    trimmed.to_string()
}

fn get_queue_storage_paths(app: &AppHandle) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if resolve_portable_data_dir().is_none() {
        let dl_dir = resolve_default_downloads_dir(app);
        if dl_dir.exists() {
            paths.push(dl_dir.join("duke_downloads_queue.json"));
        }
    }
    if let Ok(appdata_p) = get_app_data_file(app, "downloads.json") {
        paths.push(appdata_p);
    }
    paths
}

pub(crate) fn normalize_game_entry(entry: &mut GameEntry) {
    entry.is_installed = Path::new(&entry.exe_path).exists();
    if entry.source.trim().is_empty() {
        entry.source = if entry.platform.eq_ignore_ascii_case("Steam") {
            "Steam".to_string()
        } else {
            "Portable".to_string()
        };
    }
    if entry.acquisition_source.trim().is_empty() {
        let src_lower = entry.source.trim().to_ascii_lowercase();
        entry.acquisition_source = match src_lower.as_str() {
            "turbo downloader" | "turbo_downloader" | "downloader" => {
                "turbo_downloader".to_string()
            }
            "steam" => "steam".to_string(),
            "epic" | "epic games" => "epic".to_string(),
            "gog" | "gog galaxy" => "gog".to_string(),
            "xbox" | "xbox game pass" => "xbox".to_string(),
            "duke swarm" | "swarm" => "swarm".to_string(),
            _ => "manual".to_string(),
        };
    }
    if entry.imported_playtime_secs == 0 && entry.duke_playtime_secs == 0 && entry.playtime_seconds > 0 {
        entry.duke_playtime_secs = entry.playtime_seconds;
    }
    entry.playtime_seconds = entry
        .imported_playtime_secs
        .saturating_add(entry.duke_playtime_secs);
    if entry.app_id.is_none() {
        if let Some(sid) = entry.steam_app_id {
            entry.app_id = u32::try_from(sid).ok();
        }
    }
    if entry.steam_app_id.is_none() {
        if let Some(aid) = entry.app_id {
            entry.steam_app_id = Some(aid as u64);
        }
    }
    if entry.last_played_ts.is_none() && entry.last_played.is_some() {
        entry.last_played_ts = entry.last_played;
    }
    if entry.last_played.is_none() && entry.last_played_ts.is_some() {
        entry.last_played = entry.last_played_ts;
    }
    if entry.steam_price_formatted.is_none() {
        if let Some(cents) = entry.steam_price_cents {
            entry.steam_price_formatted = Some(format!("${:.2}", cents as f64 / 100.0));
        }
    }
    if let Some(ref args) = entry.launch_args {
        if args.trim().is_empty() {
            entry.launch_args = None;
        }
    }
}

fn get_library_io_lock() -> &'static Mutex<()> {
    LIBRARY_IO_LOCK.get_or_init(|| Mutex::new(()))
}

fn get_queue_io_lock() -> &'static Mutex<()> {
    QUEUE_IO_LOCK.get_or_init(|| Mutex::new(()))
}

fn get_settings_io_lock() -> &'static Mutex<()> {
    SETTINGS_IO_LOCK.get_or_init(|| Mutex::new(()))
}

fn get_catalog_io_lock() -> &'static Mutex<()> {
    CATALOG_IO_LOCK.get_or_init(|| Mutex::new(()))
}

pub(crate) fn reload_queue_store_from_disk(app: &AppHandle) {
    let Ok(dl_path) = get_app_data_file(app, "downloads.json") else {
        return;
    };
    let Ok(raw) = self_heal::read_json_with_bak_recovery(&dl_path) else {
        return;
    };
    if let Ok(mut parsed) = serde_json::from_str::<Vec<DownloadJob>>(&raw) {
        for j in &mut parsed {
            reconcile_job_bytes_from_disk(j);
        }
        if let Ok(mut guard) = get_queue_store().lock() {
            *guard = parsed;
        }
    }
}

pub(crate) fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    self_heal::atomic_write_with_backup(path, bytes)
}

pub(crate) fn atomic_write_string(path: &Path, contents: &str) -> Result<(), String> {
    atomic_write_bytes(path, contents.as_bytes())
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

pub(crate) fn load_catalog_cache_standalone() -> CatalogCacheFile {
    let _guard = get_catalog_io_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let _ = catalog_mmap::ensure_catalog_mapped_standalone();
    let mmap_cache = catalog_mmap::load_catalog_via_mmap_standalone();
    if !mmap_cache.entries.is_empty() || mmap_cache.fetched_at > 0 {
        if let Ok(legacy_path) = get_app_data_file_standalone("catalog_cache.json") {
            if legacy_path.is_file() {
                let _ = fs::remove_file(&legacy_path);
            }
        }
        return mmap_cache;
    }
    let Ok(path) = get_app_data_file_standalone("catalog_cache.json") else {
        return CatalogCacheFile::default();
    };
    if !path.exists() {
        return CatalogCacheFile::default();
    }
    let Ok(raw) = fs::read_to_string(&path) else {
        return CatalogCacheFile::default();
    };
    if raw.trim().is_empty() {
        let _ = fs::remove_file(&path);
        return CatalogCacheFile::default();
    }
    let parsed = serde_json::from_str::<CatalogCacheFile>(&raw).unwrap_or_default();
    if !parsed.entries.is_empty() {
        if let Ok(dkcat_path) = get_app_data_file_standalone("catalog.dkcat") {
            let _ = catalog_mmap::atomic_rebuild_dkcat(&dkcat_path, &parsed);
        }
    }
    let _ = fs::remove_file(&path);
    parsed
}

fn persist_cover_to_disk_cache_standalone(game_id: &str, cover_str: &str) -> String {
    let trimmed = cover_str.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if !trimmed.starts_with("data:image/") {
        if trimmed.len() <= 2048 {
            return trimmed.to_string();
        }
        return String::new();
    }
    let max_b64_chars = (MAX_COVER_IMAGE_BYTES * 4) / 3 + 128;
    if trimmed.len() > max_b64_chars {
        return String::new();
    }
    let safe_id: String = game_id
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .take(80)
        .collect();
    if safe_id.is_empty() {
        return trimmed.to_string();
    }
    if let Ok(covers_dir) = get_covers_dir_standalone() {
        let cache_file = covers_dir.join(format!("{}.cover", safe_id));
        if atomic_write_string(&cache_file, trimmed).is_ok() {
            return format!("cover-cache://{}", safe_id);
        }
    }
    trimmed.to_string()
}

fn hydrate_cover_from_disk_cache_standalone(game_id: &str, stored_cover: &str) -> String {
    let trimmed = stored_cover.trim();
    if let Some(cache_key) = trimmed.strip_prefix("cover-cache://") {
        if let Ok(covers_dir) = get_covers_dir_standalone() {
            let cache_file = covers_dir.join(format!("{}.cover", cache_key.trim()));
            if let Ok(data_uri) = fs::read_to_string(&cache_file) {
                if !data_uri.trim().is_empty() {
                    return data_uri;
                }
            }
        }
    }
    let safe_id: String = game_id
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .take(80)
        .collect();
    if !safe_id.is_empty() && trimmed.is_empty() {
        if let Ok(covers_dir) = get_covers_dir_standalone() {
            let cache_file = covers_dir.join(format!("{}.cover", safe_id));
            if cache_file.exists() {
                if let Ok(data_uri) = fs::read_to_string(&cache_file) {
                    if !data_uri.trim().is_empty() {
                        return data_uri;
                    }
                }
            }
        }
    }
    trimmed.to_string()
}

pub(crate) fn load_library_standalone() -> Result<Vec<GameEntry>, String> {
    let _guard = get_library_io_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let path = get_app_data_file_standalone("library.json")?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read library.json: {}", e))?;
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut entries: Vec<GameEntry> = serde_json::from_str(&raw)
        .map_err(|e| format!("Failed to parse library.json: {}", e))?;
    for entry in &mut entries {
        normalize_game_entry(entry);
        entry.cover_url = hydrate_cover_from_disk_cache_standalone(&entry.id, &entry.cover_url);
    }
    Ok(entries)
}

pub(crate) fn save_library_standalone(entries: &[GameEntry]) -> Result<(), String> {
    let _guard = get_library_io_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let path = get_app_data_file_standalone("library.json")?;
    let mut disk_entries: Vec<GameEntry> = entries.to_vec();
    for entry in &mut disk_entries {
        if entry.cover_url.starts_with("data:image/") {
            entry.cover_url = persist_cover_to_disk_cache_standalone(&entry.id, &entry.cover_url);
        }
    }
    let serialized = serde_json::to_string_pretty(&disk_entries)
        .map_err(|e| format!("Failed to serialize library: {}", e))?;
    atomic_write_string(&path, &serialized)
}

fn load_settings_from_disk(app: &AppHandle) -> LauncherSettings {
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

fn save_settings_to_disk(app: &AppHandle, settings: &LauncherSettings) -> Result<(), String> {
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

fn load_catalog_cache_from_disk(app: &AppHandle) -> CatalogCacheFile {
    let _guard = get_catalog_io_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let _ = catalog_mmap::ensure_catalog_mapped_standalone();
    if let Ok(dkcat_path) = get_app_data_file(app, "catalog.dkcat") {
        if dkcat_path.is_file() {
            if let Ok(mapped) = catalog_mmap::MappedCatalog::open(&dkcat_path) {
                if let Ok(legacy_path) = get_app_data_file(app, "catalog_cache.json") {
                    if legacy_path.is_file() {
                        let _ = fs::remove_file(&legacy_path);
                    }
                }
                return mapped.to_catalog_cache_file();
            }
        }
    }
    let Ok(path) = get_app_data_file(app, "catalog_cache.json") else {
        return CatalogCacheFile::default();
    };
    if !path.exists() {
        return CatalogCacheFile::default();
    }
    let Ok(raw) = fs::read_to_string(&path) else {
        return CatalogCacheFile::default();
    };
    if raw.trim().is_empty() {
        let _ = fs::remove_file(&path);
        return CatalogCacheFile::default();
    }
    let parsed = serde_json::from_str::<CatalogCacheFile>(&raw).unwrap_or_default();
    if !parsed.entries.is_empty() {
        if let Ok(dkcat_path) = get_app_data_file(app, "catalog.dkcat") {
            let _ = catalog_mmap::atomic_rebuild_dkcat(&dkcat_path, &parsed);
        }
    }
    let _ = fs::remove_file(&path);
    parsed
}

fn save_catalog_cache_to_disk(app: &AppHandle, cache: &CatalogCacheFile) -> Result<(), String> {
    let _guard = get_catalog_io_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let dkcat_path = get_app_data_file(app, "catalog.dkcat")?;
    catalog_mmap::atomic_rebuild_dkcat(&dkcat_path, cache)?;
    Ok(())
}

pub(crate) fn load_library_lightweight(app: &AppHandle) -> Result<Vec<GameEntry>, String> {
    let _guard = get_library_io_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let path = get_app_data_file(app, "library.json")?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read library.json: {}", e))?;
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut entries: Vec<GameEntry> = serde_json::from_str(&raw)
        .map_err(|e| format!("Failed to parse library.json: {}", e))?;
    for entry in &mut entries {
        normalize_game_entry(entry);
    }
    Ok(entries)
}

fn load_library_from_disk(app: &AppHandle) -> Result<Vec<GameEntry>, String> {
    let _guard = get_library_io_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let path = get_app_data_file(app, "library.json")?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let raw = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read library.json: {}", e))?;

    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }

    let mut entries: Vec<GameEntry> = serde_json::from_str(&raw)
        .map_err(|e| format!("Failed to parse library.json: {}", e))?;

    let mut migrated_inline_covers = false;
    for entry in &mut entries {
        normalize_game_entry(entry);
        if entry.cover_url.starts_with("data:image/") {
            let _ = persist_cover_to_disk_cache(app, &entry.id, &entry.cover_url);
            migrated_inline_covers = true;
        } else {
            entry.cover_url = hydrate_cover_from_disk_cache(app, &entry.id, &entry.cover_url);
        }
    }

    if migrated_inline_covers {
        drop(_guard);
        let _ = save_library_to_disk(app, &entries);
    }

    Ok(entries)
}

fn save_library_to_disk(app: &AppHandle, entries: &[GameEntry]) -> Result<(), String> {
    let _guard = get_library_io_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let path = get_app_data_file(app, "library.json")?;
    let mut disk_entries: Vec<GameEntry> = entries.to_vec();
    for entry in &mut disk_entries {
        if entry.cover_url.starts_with("data:image/") {
            entry.cover_url = persist_cover_to_disk_cache(app, &entry.id, &entry.cover_url);
        }
    }

    let serialized = serde_json::to_string_pretty(&disk_entries)
        .map_err(|e| format!("Failed to serialize library: {}", e))?;

    atomic_write_string(&path, &serialized)
}

fn segment_file_path(part_base: &str, index: usize) -> PathBuf {
    PathBuf::from(format!("{}.seg{}", part_base, index))
}

fn reconcile_job_bytes_from_disk(job: &mut DownloadJob) {
    let final_p = PathBuf::from(&job.final_path);
    if final_p.exists() && job.status == "COMPLETED" {
        if let Ok(meta) = fs::metadata(&final_p) {
            job.downloaded_bytes = meta.len();
            job.total_bytes = meta.len();
            job.progress_pct = 100.0;
            return;
        }
    }

    let part_file_p = PathBuf::from(&job.part_path);
    let part_file_len = fs::metadata(&part_file_p).map(|m| m.len()).unwrap_or(0);

    if !job.segments.is_empty() {
        if part_file_p.exists() {
            // Pre-allocated `.part` mode: `seg.current_byte` holds the flushed `seek_write` cursor.
            let mut total_downloaded = 0u64;
            for seg in &mut job.segments {
                let bounded_curr = if seg.end_byte != u64::MAX {
                    seg.current_byte
                        .clamp(seg.start_byte, seg.end_byte.saturating_add(1))
                } else {
                    seg.current_byte.max(seg.start_byte)
                };
                seg.current_byte = bounded_curr;
                seg.completed = seg.end_byte != u64::MAX && seg.current_byte > seg.end_byte;
                total_downloaded = total_downloaded.saturating_add(seg.current_byte.saturating_sub(seg.start_byte));
            }
            if part_file_len > 0 {
                total_downloaded = total_downloaded.min(part_file_len.max(total_downloaded));
            }
            job.downloaded_bytes = total_downloaded;
        } else {
            // Fallback reconciliation for legacy `.segN` files or missing `.part` files on disk.
            let mut total_on_disk = 0u64;
            for seg in &mut job.segments {
                let seg_p = segment_file_path(&job.part_path, seg.index);
                let seg_len = fs::metadata(&seg_p).map(|m| m.len()).unwrap_or(0);
                let max_seg_len = if seg.end_byte != u64::MAX {
                    (seg.end_byte - seg.start_byte) + 1
                } else {
                    seg_len
                };
                let clamped_len = seg_len.min(max_seg_len);
                seg.current_byte = seg.start_byte + clamped_len;
                seg.completed = seg.end_byte != u64::MAX && seg.current_byte > seg.end_byte;
                total_on_disk += clamped_len;
            }
            job.downloaded_bytes = total_on_disk;
        }
    } else if part_file_p.exists() {
        job.downloaded_bytes = part_file_len;
    } else {
        let single_p = segment_file_path(&job.part_path, 0);
        if single_p.exists() {
            job.downloaded_bytes = fs::metadata(&single_p).map(|m| m.len()).unwrap_or(0);
        } else {
            job.downloaded_bytes = 0;
        }
    }

    if job.total_bytes > 0 {
        job.progress_pct =
            ((job.downloaded_bytes as f64 / job.total_bytes as f64) * 100.0).min(99.9);
    }
}

fn is_job_auto_cleanable(job: &DownloadJob) -> bool {
    job.status == "COMPLETED"
        && job.registered_in_library
        && job.verdict.as_deref() != Some("DANGER")
}

fn prune_auto_cleanable_jobs(jobs: &mut Vec<DownloadJob>, auto_clean: bool) -> bool {
    if !auto_clean {
        return false;
    }
    let before = jobs.len();
    jobs.retain(|job| !is_job_auto_cleanable(job));
    jobs.len() != before
}

fn schedule_auto_clean_completed_job(app: AppHandle, download_id: String) {
    if !load_settings_from_disk(&app).auto_clean_completed {
        return;
    }

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(2500)).await;
        if !load_settings_from_disk(&app).auto_clean_completed {
            return;
        }

        let remaining: Option<Vec<DownloadJob>> = {
            let mut guard = get_queue_store().lock().unwrap_or_else(|p| p.into_inner());
            let before = guard.len();
            guard.retain(|j| !(j.download_id == download_id && is_job_auto_cleanable(j)));
            if guard.len() != before {
                persist_queue_to_disk(&app, &guard);
                Some(guard.clone())
            } else {
                None
            }
        };

        if let Some(updated_queue) = remaining {
            let _ = app.emit("download-queue-updated", &updated_queue);
            let _ = app.emit(
                "download-auto-cleaned",
                serde_json::json!({
                    "downloadId": download_id,
                    "remaining": updated_queue.len()
                }),
            );
            trim_process_working_set();
        }
    });
}

fn persist_queue_to_disk(app: &AppHandle, jobs: &[DownloadJob]) {
    let _io_guard = get_queue_io_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    if let Ok(serialized) = serde_json::to_string_pretty(jobs) {
        for path in get_queue_storage_paths(app) {
            let _ = atomic_write_string(&path, &serialized);
        }
    }
}

fn ensure_queue_initialized(app: &AppHandle) -> Vec<DownloadJob> {
    let auto_clean = load_settings_from_disk(app).auto_clean_completed;
    let mut guard = get_queue_store().lock().unwrap_or_else(|p| p.into_inner());
    if !guard.is_empty() {
        if prune_auto_cleanable_jobs(&mut guard, auto_clean) {
            persist_queue_to_disk(app, &guard);
        }
        return guard.clone();
    }

    let mut loaded_jobs: Vec<DownloadJob> = Vec::new();
    for path in get_queue_storage_paths(app) {
        if path.exists() {
            let raw = fs::read_to_string(&path).unwrap_or_default();
            if let Ok(parsed) = serde_json::from_str::<Vec<DownloadJob>>(&raw) {
                if !parsed.is_empty() {
                    loaded_jobs = parsed;
                    break;
                }
            }
        }
    }

    let pause_map = get_pause_map().lock().unwrap_or_else(|p| p.into_inner());

    for job in &mut loaded_jobs {
        reconcile_job_bytes_from_disk(job);

        if job.status != "COMPLETED" && !pause_map.contains_key(&job.download_id) {
            job.status = "PAUSED".to_string();
            job.speed_mbps = 0.0;
            job.eta_secs = None;
            job.message = if job.downloaded_bytes > 0 {
                format!(
                    "Saved at {} - Click RESUME to continue",
                    format_bytes(job.downloaded_bytes)
                )
            } else {
                "Saved in queue - Click RESUME to start".to_string()
            };
        }
    }

    let pruned = prune_auto_cleanable_jobs(&mut loaded_jobs, auto_clean);
    *guard = loaded_jobs.clone();
    if !loaded_jobs.is_empty() || pruned {
        persist_queue_to_disk(app, &loaded_jobs);
    }
    loaded_jobs
}

fn update_job_in_memory_and_disk(app: &AppHandle, updated: &DownloadJob, write_disk: bool) {
    let mut guard = get_queue_store().lock().unwrap_or_else(|p| p.into_inner());
    if let Some(existing) = guard
        .iter_mut()
        .find(|j| j.download_id == updated.download_id)
    {
        *existing = updated.clone();
    } else {
        guard.insert(0, updated.clone());
    }
    if write_disk {
        persist_queue_to_disk(app, &guard);
    }
}

fn get_http_client() -> Client {
    duke_core::net_doh::get_configured_http_client()
}

/// Rotates cached DoH edge IPs for a URL's host so workers can fail over across CDN edge nodes when throttled.
fn rotate_doh_cached_ips(raw_url: &str) {
    duke_core::net_doh::rotate_doh_cached_ips(raw_url);
}

/// Builds a `reqwest::Client` pre-populated with encrypted DoH `SocketAddr` mappings via `duke-core`.
pub(crate) async fn build_doh_client_for_url(
    raw_url: &str,
    is_burst: bool,
    follow_redirects: bool,
) -> Client {
    if let Ok(parsed) = Url::parse(raw_url) {
        if let Some(host) = parsed.host_str() {
            let lower_host = host.to_lowercase();
            if lower_host.contains("bzzhr.to")
                || lower_host.contains("buzzheavier.com")
                || lower_host.contains("bhvr.net")
            {
                let _ = duke_core::net_doh::resolve_domain_doh("ts.bzzhr.to", 443).await;
                let _ = duke_core::net_doh::resolve_domain_doh("bzzhr.to", 443).await;
                let _ = duke_core::net_doh::resolve_domain_doh("buzzheavier.com", 443).await;
            } else if lower_host.contains("gofile.io") {
                let _ = duke_core::net_doh::resolve_domain_doh("api.gofile.io", 443).await;
            } else if lower_host.contains("fileditch") {
                let _ = duke_core::net_doh::resolve_domain_doh("fileditchfiles.me", 443).await;
            }
        }
    }
    duke_core::net_doh::build_doh_client_for_url(raw_url, is_burst, follow_redirects).await
}

async fn build_metadata_doh_client(raw_url: &str) -> Client {
    let timeout = Duration::from_secs(HTTP_METADATA_TIMEOUT_SECS);
    duke_core::net_doh::build_doh_client_with_timeouts(raw_url, timeout, timeout, true).await
}

pub(crate) async fn fetch_doh_json(raw_url: &str) -> Result<Value, String> {
    let client = build_metadata_doh_client(raw_url).await;
    let resp = client
        .get(raw_url)
        .header(ACCEPT, "application/json, text/plain, */*")
        .timeout(Duration::from_secs(HTTP_METADATA_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| format!("Request failed for {}: {}", raw_url, e))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {} for {}", resp.status().as_u16(), raw_url));
    }

    resp.json::<Value>()
        .await
        .map_err(|e| format!("Invalid JSON from {}: {}", raw_url, e))
}

pub(crate) async fn fetch_verified_image_data_uri(url: &str) -> Option<String> {
    let client = get_http_client();
    let resp = client
        .get(url)
        .header(ACCEPT, "image/webp,image/apng,image/*,*/*;q=0.8")
        .timeout(Duration::from_secs(6))
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let ctype = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_lowercase();

    if ctype.contains("text/html") || ctype.contains("application/json") {
        return None;
    }

    let bytes = resp.bytes().await.ok()?;
    // Cap cover art payload size to MAX_COVER_IMAGE_BYTES (256 KB) to prevent large Base64 strings from bloating DOM/IPC memory
    if bytes.len() < 1200 || bytes.len() > MAX_COVER_IMAGE_BYTES {
        return None;
    }

    let mime = if ctype.contains("png") {
        "image/png"
    } else if ctype.contains("webp") {
        "image/webp"
    } else {
        "image/jpeg"
    };

    Some(format!("data:{};base64,{}", mime, encode_base64(&bytes)))
}

async fn fetch_steam_app_cover_verified(app_id: u64) -> (Option<String>, Option<String>) {
    // Try compact widescreen header.jpg (~35 KB) first for fast, low-RAM caching
    let fast_header = format!(
        "https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/{}/header.jpg",
        app_id
    );
    let details_url = format!(
        "https://store.steampowered.com/api/appdetails?appids={}&l=english&cc=US",
        app_id
    );

    if let Ok(resp) = get_http_client()
        .get(&details_url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        if let Ok(json_val) = resp.json::<Value>().await {
            if let Some(data) = json_val.get(app_id.to_string()).and_then(|e| e.get("data")) {
                let official_name = data
                    .get("name")
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string());

                for field in ["header_image", "capsule_image", "capsule_imagev5"] {
                    if let Some(img_url) = data.get(field).and_then(|u| u.as_str()) {
                        if let Some(data_uri) = fetch_verified_image_data_uri(img_url).await {
                            return (Some(data_uri), official_name);
                        }
                    }
                }
            }
        }
    }

    if let Some(data_uri) = fetch_verified_image_data_uri(&fast_header).await {
        return (Some(data_uri), None);
    }

    (None, None)
}

fn extract_exe_icon_data_uri(exe_path: &str) -> Option<String> {
    let escaped = exe_path.replace('\'', "''");
    let ps_script = format!(
        "Add-Type -AssemblyName System.Drawing; $ico = [System.Drawing.Icon]::ExtractAssociatedIcon('{}'); if ($ico) {{ $bmp = $ico.ToBitmap(); $ms = New-Object System.IO.MemoryStream; $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png); [Convert]::ToBase64String($ms.ToArray()) }}",
        escaped
    );

    let mut cmd = Command::new("powershell");
    cmd.creation_flags(CREATE_NO_WINDOW);
    if let Ok(output) = cmd
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps_script])
        .output()
    {
        let b64 = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if b64.len() > 100 && b64.len() < 512 * 1024 {
            return Some(format!("data:image/png;base64,{}", b64));
        }
    }
    None
}

fn detect_steam_appid_in_folder(install_dir: &Path) -> Option<u64> {
    if !install_dir.exists() {
        return None;
    }
    for entry in WalkDir::new(install_dir)
        .max_depth(4)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let fname = entry.file_name().to_string_lossy().to_lowercase();
        if fname == "steam_appid.txt" {
            if let Ok(content) = fs::read_to_string(entry.path()) {
                if let Ok(id) = content.trim().parse::<u64>() {
                    if id > 0 {
                        return Some(id);
                    }
                }
            }
        } else if fname == "steam_emu.ini"
            || fname == "onlinefix.ini"
            || fname == "tenoke.ini"
            || fname == "codex.ini"
            || fname == "goldberg.ini"
            || fname == " cream_api.ini"
            || fname == "cream_api.ini"
        {
            if let Ok(content) = fs::read_to_string(entry.path()) {
                for line in content.lines() {
                    let trimmed = line.trim().to_lowercase();
                    if trimmed.starts_with("appid") && trimmed.contains('=') {
                        if let Some(val_str) = trimmed.split('=').nth(1) {
                            if let Ok(id) = val_str.trim().parse::<u64>() {
                                if id > 0 {
                                    return Some(id);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

async fn resolve_steam_appid_by_title(title: &str) -> Option<(u32, Option<String>)> {
    let clean = clean_game_title(title);
    if clean.is_empty() || clean == "Unknown Game" {
        return None;
    }
    let encoded = urlencoding::encode(&clean);
    let url = format!(
        "https://store.steampowered.com/api/storesearch/?term={}&l=english&cc=US",
        encoded
    );
    let json_val = fetch_doh_json(&url).await.ok()?;
    let items = json_val.get("items")?.as_array()?;
    let first = items.first()?;
    let id = first.get("id")?.as_u64()?;
    let name = first
        .get("name")
        .and_then(|n| n.as_str())
        .map(|s| s.to_string());
    Some((u32::try_from(id).ok()?, name))
}

fn extract_steam_price_from_appdetails_data(data: &Value) -> (u32, String) {
    let is_free = data
        .get("is_free")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if is_free {
        return (0, "$0.00".to_string());
    }
    if let Some(po) = data.get("price_overview") {
        let initial_cents = po
            .get("initial")
            .and_then(|v| v.as_u64())
            .filter(|&c| c > 0)
            .or_else(|| po.get("final").and_then(|v| v.as_u64()))
            .unwrap_or(0) as u32;
        return (initial_cents, format!("${:.2}", initial_cents as f64 / 100.0));
    }
    (0, "$0.00".to_string())
}

async fn fetch_steam_app_retail_price(appid: u32) -> (u32, String) {
    if appid == 0 {
        return (0, "$0.00".to_string());
    }
    let details_url = format!(
        "https://store.steampowered.com/api/appdetails?appids={}&cc=us&l=en",
        appid
    );
    if let Ok(json_val) = fetch_doh_json(&details_url).await {
        if let Some(data) = json_val.get(appid.to_string()).and_then(|v| v.get("data")) {
            return extract_steam_price_from_appdetails_data(data);
        }
    }
    (0, "$0.00".to_string())
}

async fn smart_find_game_cover(
    app: &AppHandle,
    cache_id: &str,
    title: &str,
    exe_path: &str,
    install_dir: &str,
) -> (String, Option<u64>, Option<String>) {
    let dir_path = Path::new(install_dir);
    let detected_app_id = if dir_path.exists() {
        detect_steam_appid_in_folder(dir_path)
    } else {
        None
    };

    let (inferred_pid, plat_hint) =
        artwork::infer_game_platform_hint(title, exe_path, "", "");
    let cleaned_title = clean_game_title(title);
    let lookup_title = if !cleaned_title.is_empty() {
        cleaned_title
    } else {
        title.trim().to_string()
    };

    if let (Ok(covers_dir), Ok(icons_dir)) = (
        get_covers_dir(app),
        sidebar_icons::get_sidebar_icons_dir(app),
    ) {
        let api_key = load_settings_from_disk(app)
            .steamgriddb_api_key
            .unwrap_or_default();
        let req = artwork::CoverResolveRequest {
            cache_id: cache_id.to_string(),
            title: lookup_title,
            platform_id: inferred_pid,
            platform_hint: plat_hint,
            steam_app_id: detected_app_id.and_then(|v| u32::try_from(v).ok()),
            existing_cover_url: None,
            steamgriddb_api_key: api_key,
            sidebar_icon_id: Some(cache_id.to_string()),
        };
        if let Some(outcome) =
            artwork::resolve_and_cache_cover_in_dirs(&req, &covers_dir, &icons_dir).await
        {
            return (
                outcome.cover_path.to_string_lossy().to_string(),
                detected_app_id,
                None,
            );
        }
    }

    if let Some(aid) = detected_app_id {
        let (cover_opt, name_opt) = fetch_steam_app_cover_verified(aid).await;
        if let Some(valid_cover) = cover_opt {
            return (valid_cover, Some(aid), name_opt);
        }
    }

    if let Some(exe_ico) = extract_exe_icon_data_uri(exe_path) {
        return (exe_ico, None, None);
    }

    (String::new(), detected_app_id, None)
}

fn derive_filename(url: &str, disposition: Option<&str>, suggested: Option<&str>) -> String {
    if let Some(disp) = disposition {
        if let Some(idx) = disp.find("filename*=") {
            let sub = &disp[idx + "filename*=".len()..];
            let cleaned = sub
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .trim_start_matches("UTF-8''")
                .trim_matches(|c| c == '"' || c == '\'');
            if let Ok(decoded) = urlencoding::decode(cleaned) {
                if !decoded.trim().is_empty() {
                    return sanitize_filename(&decoded);
                }
            }
        }
        if let Some(idx) = disp.find("filename=") {
            let sub = &disp[idx + "filename=".len()..];
            let cleaned = sub
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches(|c| c == '"' || c == '\'');
            if !cleaned.is_empty() {
                return sanitize_filename(cleaned);
            }
        }
    }

    if let Some(s) = suggested {
        let trimmed = s.trim();
        if !trimmed.is_empty()
            && trimmed.contains('.')
            && !trimmed.starts_with("Queued_")
            && !trimmed.starts_with("DukeDownload_")
        {
            return sanitize_filename(trimmed);
        }
    }

    let without_query = url.split('?').next().unwrap_or(url);
    if let Some(last_seg) = without_query.rsplit('/').next() {
        let decoded = urlencoding::decode(last_seg)
            .map(|s| s.into_owned())
            .unwrap_or_else(|_| last_seg.to_string());
        if !decoded.is_empty()
            && decoded.contains('.')
            && !decoded.ends_with(".html")
            && !decoded.ends_with(".php")
            && !decoded.ends_with("/download")
        {
            return sanitize_filename(&decoded);
        }
    }

    format!(
        "DukeDownload_{}.rar",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    )
}

pub(crate) fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() {
        "download.rar".to_string()
    } else {
        trimmed.to_string()
    }
}

fn parse_host_html_metadata(html: &str) -> (Option<String>, Option<String>) {
    let mut filename: Option<String> = None;
    let mut hx_download_path: Option<String> = None;

    if let Some(t_start) = html.find("<title>") {
        let rest = &html[t_start + 7..];
        if let Some(t_end) = rest.find("</title>") {
            let raw_title = rest[..t_end]
                .split(" - ")
                .next()
                .unwrap_or("")
                .split(" | ")
                .next()
                .unwrap_or("")
                .trim();
            if raw_title.contains('.') {
                filename = Some(raw_title.to_string());
            }
        }
    }

    for marker in ["hx-get=\"", "hx-get='"] {
        let mut slice = html;
        while let Some(pos) = slice.find(marker) {
            let rest = &slice[pos + marker.len()..];
            if let Some(end) = rest.find(['"', '\'']) {
                let candidate = &rest[..end];
                if candidate.contains("download") {
                    hx_download_path = Some(candidate.to_string());
                    break;
                }
                slice = &rest[end..];
            } else {
                break;
            }
        }
        if hx_download_path.is_some() {
            break;
        }
    }

    (filename, hx_download_path)
}

fn collect_cookies(resp: &reqwest::Response) -> String {
    let mut pairs = Vec::new();
    for val in resp.headers().get_all(SET_COOKIE) {
        if let Ok(s) = val.to_str() {
            if let Some(first_part) = s.split(';').next() {
                let trimmed = first_part.trim();
                if !trimmed.is_empty() {
                    pairs.push(trimmed.to_string());
                }
            }
        }
    }
    pairs.join("; ")
}

fn is_direct_cdn_binary_url(raw_url: &str) -> bool {
    let lower = raw_url.trim().to_lowercase();
    if lower.contains("ts.bzzhr.to/d/")
        || lower.contains("bzzhr.to/d/")
        || lower.contains("buzzheavier.com/d/")
        || lower.contains("bhvr.net/d/")
        || lower.contains("pixeldrain.com/api/file/")
        || (lower.contains("fileditchfiles.me/") && !lower.ends_with(".html"))
    {
        return true;
    }

    let without_query = lower.split('?').next().unwrap_or(&lower);
    [
        ".rar",
        ".7z",
        ".zip",
        ".iso",
        ".exe",
        ".msi",
        ".bin",
        ".tar",
        ".tar.zst",
        ".tzst",
        ".gz",
        ".tgz",
        ".rvz",
        ".chd",
        ".3ds",
        ".cci",
        ".nsp",
        ".xci",
        ".gba",
        ".nds",
        ".z64",
        ".sfc",
        ".nes",
        ".7z.001",
        ".zip.001",
    ]
    .iter()
    .any(|ext| without_query.ends_with(ext))
}

/// Probes a stream URL with a fast `Range: bytes=0-0` request over DoH.
/// Returns `Some(Response)` if the server responds with `206 Partial Content` or `200 OK` and a non-HTML Content-Type.
async fn probe_direct_stream(
    url: &str,
    referer: &str,
    cookie: &str,
    user_agent: &str,
) -> Option<reqwest::Response> {
    let trimmed = url.trim();
    if trimmed.is_empty()
        || (!trimmed.starts_with("https://") && !trimmed.starts_with("http://"))
    {
        return None;
    }

    let client = build_doh_client_for_url(trimmed, false, true).await;
    let ua = if user_agent.trim().is_empty() {
        get_session_user_agent()
    } else {
        user_agent.trim()
    };

    let mut req = client
        .get(trimmed)
        .header(USER_AGENT, ua)
        .header(RANGE, "bytes=0-0")
        .timeout(Duration::from_secs(6));

    if !referer.trim().is_empty() {
        req = req.header(REFERER, referer.trim());
    }
    if !cookie.trim().is_empty() {
        req = req.header(COOKIE, cookie.trim());
    }

    let resp = req.send().await.ok()?;
    let status = resp.status();
    if !(status.as_u16() == 206 || status.is_success()) {
        return None;
    }

    let ctype = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    if ctype.contains("text/html") {
        return None;
    }

    Some(resp)
}

async fn try_resolve_gofile(url: &str) -> Option<ResolvedStreamInfo> {
    let id = url.split("/d/").nth(1)?.split(['/', '?', '#']).next()?.trim();
    if id.is_empty() {
        return None;
    }

    let ua = get_session_user_agent().to_string();
    let client = build_doh_client_for_url("https://api.gofile.io/accounts", false, true).await;
    let acc_resp = client
        .post("https://api.gofile.io/accounts")
        .send()
        .await
        .ok()?;
    let acc_json: Value = acc_resp.json().await.ok()?;
    let token = acc_json.get("data")?.get("token")?.as_str()?.to_string();

    let mut wt = "4fd6sg89d7s6".to_string();
    if let Ok(js_resp) = client.get("https://gofile.io/dist/js/global.js").send().await {
        if let Ok(js_text) = js_resp.text().await {
            if let Some(wt_idx) = js_text.find("appdata.wt = \"") {
                let rest = &js_text[wt_idx + 14..];
                if let Some(end) = rest.find('"') {
                    wt = rest[..end].to_string();
                }
            }
        }
    }

    let content_url = format!("https://api.gofile.io/contents/{}?wt={}", id, wt);
    let content_resp = client
        .get(&content_url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .ok()?;

    let content_json: Value = content_resp.json().await.ok()?;
    let children = content_json
        .get("data")?
        .get("children")?
        .as_object()?;

    for (_key, child) in children {
        if let Some(link) = child.get("link").and_then(|l| l.as_str()) {
            let name = child
                .get("name")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string());
            return Some(ResolvedStreamInfo {
                stream_url: link.to_string(),
                referer: "https://gofile.io/".to_string(),
                cookie: format!("accountToken={}", token),
                user_agent: ua,
                suggested_filename: name,
            });
        }
    }

    None
}

async fn try_resolve_pixeldrain(url: &str) -> Option<ResolvedStreamInfo> {
    let id = if let Some(rest) = url.split("pixeldrain.com/u/").nth(1) {
        rest.split(['/', '?', '#']).next()?.trim()
    } else {
        let rest = url.split("pixeldrain.com/api/file/").nth(1)?;
        rest.split(['/', '?', '#']).next()?.trim()
    };

    if id.is_empty() {
        return None;
    }

    let ua = get_session_user_agent().to_string();
    let mut suggested_filename = None;
    let info_url = format!("https://pixeldrain.com/api/file/{}/info", id);
    let client = build_doh_client_for_url(&info_url, false, true).await;
    if let Ok(resp) = client
        .get(&info_url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        if let Ok(json_val) = resp.json::<Value>().await {
            suggested_filename = json_val
                .get("name")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string());
        }
    }

    Some(ResolvedStreamInfo {
        stream_url: format!("https://pixeldrain.com/api/file/{}?download", id),
        referer: format!("https://pixeldrain.com/u/{}", id),
        cookie: String::new(),
        user_agent: ua,
        suggested_filename,
    })
}

async fn try_resolve_mediafire(url: &str) -> Option<ResolvedStreamInfo> {
    let ua = get_session_user_agent().to_string();
    let client = build_doh_client_for_url(url, false, true).await;
    let resp = client
        .get(url)
        .header(REFERER, "https://www.mediafire.com/")
        .send()
        .await
        .ok()?;

    let html = resp.text().await.ok()?;
    let (suggested_filename, _) = parse_host_html_metadata(&html);

    for marker in [
        "id=\"downloadButton\"",
        "aria-label=\"Download file\"",
        "https://download",
    ] {
        if let Some(idx) = html.find(marker) {
            let window_start = idx.saturating_sub(240);
            let window_end = (idx + 600).min(html.len());
            let snippet = &html[window_start..window_end];
            if let Some(dl_pos) = snippet.find("https://download") {
                let rest = &snippet[dl_pos..];
                let end = rest.find(['"', '\'', ' ', '>', '<']).unwrap_or(rest.len());
                let direct = &rest[..end];
                if direct.len() > 24 {
                    return Some(ResolvedStreamInfo {
                        stream_url: direct.to_string(),
                        referer: url.to_string(),
                        cookie: String::new(),
                        user_agent: ua,
                        suggested_filename,
                    });
                }
            }
        }
    }

    None
}

async fn try_resolve_fileditch(url: &str) -> Option<ResolvedStreamInfo> {
    let ua = get_session_user_agent().to_string();
    if url.contains("fileditchfiles.me/") && !url.ends_with(".html") {
        return Some(ResolvedStreamInfo {
            stream_url: url.to_string(),
            referer: "https://fileditch.com/".to_string(),
            cookie: String::new(),
            user_agent: ua,
            suggested_filename: None,
        });
    }

    let client = build_doh_client_for_url(url, false, true).await;
    let resp = client
        .get(url)
        .header(REFERER, url)
        .send()
        .await
        .ok()?;
    let ctype = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    if !ctype.contains("text/html") && resp.status().is_success() {
        return Some(ResolvedStreamInfo {
            stream_url: resp.url().to_string(),
            referer: url.to_string(),
            cookie: collect_cookies(&resp),
            user_agent: ua,
            suggested_filename: None,
        });
    }

    let html = resp.text().await.ok()?;
    let (suggested_filename, _) = parse_host_html_metadata(&html);
    for marker in ["https://fileditchfiles.me/", "https://s1.fileditch", "https://s2.fileditch"] {
        if let Some(pos) = html.find(marker) {
            let rest = &html[pos..];
            let end = rest.find(['"', '\'', ' ', '>', '<']).unwrap_or(rest.len());
            let direct = &rest[..end];
            if direct.len() > 20 {
                return Some(ResolvedStreamInfo {
                    stream_url: direct.to_string(),
                    referer: url.to_string(),
                    cookie: String::new(),
                    user_agent: ua,
                    suggested_filename,
                });
            }
        }
    }

    None
}

/// Pure-Rust Headless HTMX & API Resolver (0 MB RAM, no WebView2 needed).
/// Resolves Buzzheavier / BZZHR (`buzzheavier.com`, `bzzhr.to`, `bhvr.net`), GoFile, Pixeldrain,
/// MediaFire, FileDitch, and direct CDN binary streams in <150ms.
async fn try_resolve_native_api(raw_url: &str) -> Option<ResolvedStreamInfo> {
    let trimmed = raw_url.trim();
    let lower_target = trimmed.to_lowercase();
    let ua = get_session_user_agent().to_string();

    // 0. If already a direct CDN binary URL (e.g., ts.bzzhr.to/d/..., pixeldrain.com/api/file/...), return immediately
    if is_direct_cdn_binary_url(trimmed) {
        let default_referer = if lower_target.contains("bzzhr.to")
            || lower_target.contains("buzzheavier.com")
            || lower_target.contains("bhvr.net")
        {
            "https://bzzhr.to/".to_string()
        } else if lower_target.contains("pixeldrain.com") {
            "https://pixeldrain.com/".to_string()
        } else {
            trimmed.to_string()
        };
        return Some(ResolvedStreamInfo {
            stream_url: trimmed.to_string(),
            referer: default_referer,
            cookie: String::new(),
            user_agent: ua,
            suggested_filename: None,
        });
    }

    // 1. Native GoFile API resolution
    if lower_target.contains("gofile.io/d/") {
        if let Some(info) = try_resolve_gofile(trimmed).await {
            return Some(info);
        }
    }

    // 2. Native Pixeldrain API resolution
    if lower_target.contains("pixeldrain.com/u/")
        || lower_target.contains("pixeldrain.com/api/file/")
    {
        if let Some(info) = try_resolve_pixeldrain(trimmed).await {
            return Some(info);
        }
    }

    // 3. Native MediaFire direct scraping
    if lower_target.contains("mediafire.com/file/") || lower_target.contains("mediafire.com/?") {
        if let Some(info) = try_resolve_mediafire(trimmed).await {
            return Some(info);
        }
    }

    // 4. Native FileDitch direct scraping
    if lower_target.contains("fileditch.com/") || lower_target.contains("fileditchfiles.me/") {
        if let Some(info) = try_resolve_fileditch(trimmed).await {
            return Some(info);
        }
    }

    // 5. Pure-Rust Headless HTMX Resolver for Buzzheavier / BZZHR (`buzzheavier.com`, `bzzhr.to`, `bhvr.net`) & generic HTMX hosts
    let is_buzzheavier = lower_target.contains("bzzhr.to")
        || lower_target.contains("buzzheavier.com")
        || lower_target.contains("bhvr.net");

    let initial_referer = trimmed.to_string();

    let client = build_doh_client_for_url(trimmed, false, true).await;

    if let Ok(page_resp) = client
        .get(trimmed)
        .header(REFERER, &initial_referer)
        .send()
        .await
    {
        let redirected_url = page_resp.url().to_string();
        let landing_url = redirected_url.clone();

        let cookies = collect_cookies(&page_resp);
        let ctype = page_resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();

        if !ctype.contains("text/html") && page_resp.status().is_success() {
            return Some(ResolvedStreamInfo {
                stream_url: landing_url,
                referer: initial_referer,
                cookie: cookies,
                user_agent: ua,
                suggested_filename: None,
            });
        }

        if page_resp.status().is_success() {
            let html_body = page_resp.text().await.unwrap_or_default();
            let (suggested_filename, hx_path) = parse_host_html_metadata(&html_body);

            let lower_landing = landing_url.to_lowercase();
            let is_landing_bzzhr = lower_landing.contains("bzzhr.to")
                || lower_landing.contains("buzzheavier.com")
                || lower_landing.contains("bhvr.net");

            if is_buzzheavier || is_landing_bzzhr || hx_path.is_some() {
                let origin = landing_url
                    .split('/')
                    .take(3)
                    .collect::<Vec<_>>()
                    .join("/");

                let clean_landing_no_query = landing_url
                    .split(['?', '#'])
                    .next()
                    .unwrap_or(&landing_url)
                    .trim_end_matches('/');

                let id_segment = clean_landing_no_query
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .trim();

                let mut candidate_endpoints = Vec::new();
                if !id_segment.is_empty() && id_segment != "download" {
                    candidate_endpoints.push(format!("{}/{}/download", origin, id_segment));
                }
                let direct_append = format!("{}/download", clean_landing_no_query);
                if !candidate_endpoints.contains(&direct_append) {
                    candidate_endpoints.push(direct_append);
                }
                if let Some(p) = hx_path {
                    let hx_full = if p.starts_with("http") {
                        p
                    } else {
                        format!(
                            "{}{}{}",
                            origin,
                            if p.starts_with('/') { "" } else { "/" },
                            p
                        )
                    };
                    if !candidate_endpoints.contains(&hx_full) {
                        candidate_endpoints.insert(0, hx_full);
                    }
                }

                // Use a non-redirecting DoH client so we read `hx-redirect` or `location` in <150ms
                let htmx_client = build_doh_client_for_url(&landing_url, false, false).await;

                for dl_endpoint in candidate_endpoints {
                    let mut req = htmx_client
                        .get(&dl_endpoint)
                        .header("HX-Request", "true")
                        .header("HX-Current-URL", &landing_url)
                        .header(REFERER, &landing_url);

                    if !cookies.is_empty() {
                        req = req.header(COOKIE, &cookies);
                    }

                    if let Ok(dl_resp) = req.send().await {
                        if let Some(hx_redir) = dl_resp
                            .headers()
                            .get("hx-redirect")
                            .or_else(|| dl_resp.headers().get("HX-Redirect"))
                            .or_else(|| dl_resp.headers().get(LOCATION))
                            .and_then(|v| v.to_str().ok())
                        {
                            let clean_redir = hx_redir.trim();
                            if !clean_redir.is_empty() && !clean_redir.ends_with("/download") {
                                let resolved_cdn = if clean_redir.starts_with("http") {
                                    clean_redir.to_string()
                                } else if clean_redir.starts_with("//") {
                                    format!("https:{}", clean_redir)
                                } else {
                                    format!(
                                        "{}{}{}",
                                        origin,
                                        if clean_redir.starts_with('/') { "" } else { "/" },
                                        clean_redir
                                    )
                                };

                                return Some(ResolvedStreamInfo {
                                    stream_url: resolved_cdn,
                                    referer: landing_url,
                                    cookie: cookies,
                                    user_agent: ua,
                                    suggested_filename,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // 6. Final native probe check in case the URL responds with a binary stream on Range: bytes=0-0
    if let Some(probe_resp) = probe_direct_stream(trimmed, &initial_referer, "", &ua).await {
        return Some(ResolvedStreamInfo {
            stream_url: probe_resp.url().to_string(),
            referer: initial_referer,
            cookie: collect_cookies(&probe_resp),
            user_agent: ua,
            suggested_filename: None,
        });
    }

    None
}

const WIN11_EDGE_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/134.0.0.0 Safari/537.36 Edg/134.0.0.0";

fn get_grabber_webview_profile_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = resolve_data_root_dir(app)?.join("grabber_webview_profile");
    if !dir.exists() {
        fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create grabber WebView2 profile directory: {}", e))?;
    }
    Ok(dir)
}

fn format_cookie_list<'a, I>(cookies: I) -> String
where
    I: IntoIterator<Item = tauri::webview::Cookie<'a>>,
{
    let mut pairs: Vec<(String, String)> = Vec::new();
    for c in cookies {
        let name = c.name().trim().to_string();
        let val = c.value().trim().to_string();
        if !name.is_empty() && !pairs.iter().any(|(k, _)| k == &name) {
            pairs.push((name, val));
        }
    }
    pairs
        .into_iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("; ")
}

fn collect_cookies_from_webview<R: tauri::Runtime>(
    webview: &tauri::Webview<R>,
    primary_url: &Url,
    referer_url: Option<&str>,
) -> String {
    let mut combined = String::new();
    if let Ok(list) = webview.cookies_for_url(primary_url.clone()) {
        combined = format_cookie_list(list.iter().cloned());
    }
    if combined.is_empty() {
        if let Some(rf) = referer_url.and_then(|s| Url::parse(s).ok()) {
            if let Ok(list) = webview.cookies_for_url(rf) {
                combined = format_cookie_list(list.iter().cloned());
            }
        }
    }
    combined
}

fn collect_cookies_from_window<R: tauri::Runtime>(
    win: &tauri::WebviewWindow<R>,
    primary_url: &Url,
    referer_url: Option<&str>,
) -> String {
    let mut combined = String::new();
    if let Ok(list) = win.cookies_for_url(primary_url.clone()) {
        combined = format_cookie_list(list.iter().cloned());
    }
    if combined.is_empty() {
        if let Some(rf) = referer_url.and_then(|s| Url::parse(s).ok()) {
            if let Ok(list) = win.cookies_for_url(rf) {
                combined = format_cookie_list(list.iter().cloned());
            }
        }
    }
    combined
}

fn schedule_delayed_window_close(app: AppHandle, label: &'static str) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(120)).await;
        if let Some(win) = app.get_webview_window(label) {
            let _ = win.close();
        }
    });
}

/// Fallback WebView2 stream grabber guarded by `WEBVIEW_GRABBER_LOCK`.
/// Uses a persistent `<app_data_dir>/grabber_webview_profile` and native Win11 Edge UA with zero
/// automation overrides so Cloudflare Turnstile passes cleanly, and intercepts file downloads via
/// Tauri `.on_download` with full `Referer` + session cookies.
async fn capture_stream_via_webview(
    app: &AppHandle,
    page_url: &str,
) -> Result<ResolvedStreamInfo, String> {
    // Never open WebView2 if the URL is already a direct CDN binary link or returns binary Content-Type on probe
    if is_direct_cdn_binary_url(page_url) {
        return Ok(ResolvedStreamInfo {
            stream_url: page_url.to_string(),
            referer: page_url.to_string(),
            cookie: String::new(),
            user_agent: WIN11_EDGE_USER_AGENT.to_string(),
            suggested_filename: None,
        });
    }
    if let Some(resp) = probe_direct_stream(page_url, page_url, "", WIN11_EDGE_USER_AGENT).await {
        return Ok(ResolvedStreamInfo {
            stream_url: resp.url().to_string(),
            referer: page_url.to_string(),
            cookie: collect_cookies(&resp),
            user_agent: WIN11_EDGE_USER_AGENT.to_string(),
            suggested_filename: None,
        });
    }

    let _grabber_lock = get_webview_grabber_lock().lock().await;

    for label in ["stream-grabber", "grabber"] {
        if let Some(existing) = app.get_webview_window(label) {
            let _ = existing.close();
        }
    }
    tokio::time::sleep(Duration::from_millis(200)).await;

    let parsed_url = validate_external_http_url(page_url)?;
    let initial_url_str = parsed_url.to_string();
    let profile_dir = get_grabber_webview_profile_dir(app)?;
    let last_page_url = Arc::new(Mutex::new(initial_url_str.clone()));

    let (tx, rx) = oneshot::channel::<ResolvedStreamInfo>();
    let tx_shared = Arc::new(Mutex::new(Some(tx)));

    let tx_for_dl = Arc::clone(&tx_shared);
    let last_page_dl = Arc::clone(&last_page_url);
    let app_for_dl = app.clone();

    let tx_for_nav = Arc::clone(&tx_shared);
    let last_page_nav = Arc::clone(&last_page_url);
    let app_for_nav = app.clone();

    let app_for_new = app.clone();
    let tx_for_close = Arc::clone(&tx_shared);

    let grabber = WebviewWindowBuilder::new(
        app,
        "grabber",
        WebviewUrl::External(parsed_url),
    )
    .title("Duke Stream Grabber")
    .inner_size(960.0, 640.0)
    .center()
    .data_directory(profile_dir)
    .user_agent(WIN11_EDGE_USER_AGENT)
    .on_new_window(move |target_url, _features| {
        let target_str = target_url.as_str();
        if !is_ad_or_tracker_url(target_str) {
            if let Some(win) = app_for_new.get_webview_window("grabber") {
                let _ = win.navigate(target_url);
            }
        }
        NewWindowResponse::Deny
    })
    .on_download(move |webview, event| match event {
        DownloadEvent::Requested { url, destination } => {
            let dl_url = url.to_string();
            if is_ad_or_tracker_url(&dl_url) {
                return false;
            }
            let referer = webview
                .url()
                .ok()
                .map(|u| u.to_string())
                .filter(|s| s.starts_with("http"))
                .or_else(|| {
                    last_page_dl
                        .lock()
                        .ok()
                        .map(|g| g.clone())
                        .filter(|s| !s.is_empty())
                })
                .unwrap_or_else(|| initial_url_str.clone());
            let cookie = collect_cookies_from_webview(&webview, &url, Some(&referer));
            let suggested = destination
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string());
            let display_name = derive_filename(&dl_url, suggested.as_deref(), None);

            if let Ok(mut guard) = tx_for_dl.lock() {
                if let Some(sender) = guard.take() {
                    let _ = sender.send(ResolvedStreamInfo {
                        stream_url: dl_url.clone(),
                        referer,
                        cookie,
                        user_agent: WIN11_EDGE_USER_AGENT.to_string(),
                        suggested_filename: suggested,
                    });
                }
            }
            let _ = app_for_dl.emit(
                "grabber-download-redirect",
                DirectoryDownloadInterceptPayload {
                    url: dl_url,
                    file_name: display_name,
                    site_title: "Duke Stream Grabber".to_string(),
                },
            );
            let _ = webview.window().hide();
            schedule_delayed_window_close(app_for_dl.clone(), "grabber");
            false
        }
        DownloadEvent::Finished { .. } => false,
        _ => true,
    })
    .on_navigation(move |nav_url: &Url| -> bool {
        let nav_str = nav_url.as_str();
        if is_ad_or_tracker_url(nav_str) {
            return false;
        }

        if is_direct_cdn_binary_url(nav_str) {
            let referer = last_page_nav
                .lock()
                .ok()
                .map(|g| g.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| nav_str.to_string());
            let cookie = app_for_nav
                .get_webview_window("grabber")
                .map(|win| collect_cookies_from_window(&win, nav_url, Some(&referer)))
                .unwrap_or_default();
            let display_name = derive_filename(nav_str, None, None);

            if let Ok(mut guard) = tx_for_nav.lock() {
                if let Some(sender) = guard.take() {
                    let _ = sender.send(ResolvedStreamInfo {
                        stream_url: nav_str.to_string(),
                        referer,
                        cookie,
                        user_agent: WIN11_EDGE_USER_AGENT.to_string(),
                        suggested_filename: None,
                    });
                    let _ = app_for_nav.emit(
                        "grabber-download-redirect",
                        DirectoryDownloadInterceptPayload {
                            url: nav_str.to_string(),
                            file_name: display_name,
                            site_title: "Duke Stream Grabber".to_string(),
                        },
                    );
                    if let Some(win) = app_for_nav.get_webview_window("grabber") {
                        let _ = win.hide();
                    }
                    schedule_delayed_window_close(app_for_nav.clone(), "grabber");
                    return false;
                }
            }
        }

        if let Ok(mut guard) = last_page_nav.lock() {
            *guard = nav_str.to_string();
        }
        true
    })
    .build()
    .map_err(|e| format!("Failed to open Stream Grabber: {}", e))?;

    grabber.on_window_event(move |event| {
        if matches!(
            event,
            WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed
        ) {
            if let Ok(mut guard) = tx_for_close.lock() {
                guard.take();
            }
        }
    });

    let result = match tokio::time::timeout(Duration::from_secs(180), rx).await {
        Ok(Ok(captured)) => {
            let _ = grabber.close();
            Ok(captured)
        }
        _ => {
            let _ = grabber.close();
            Err("Stream Grabber closed before a download link was captured.".to_string())
        }
    };

    trim_process_working_set();
    result
}

fn is_ad_or_tracker_url(raw_url: &str) -> bool {
    let lower = raw_url.trim().to_lowercase();
    const BLOCKED_PATTERNS: &[&str] = &[
        "doubleclick.net",
        "googlesyndication.com",
        "googleadservices.com",
        "adservice.google",
        "popads.net",
        "popcash.net",
        "propellerads",
        "linkvertise",
        "exoclick",
        "trafficjunky",
        "adsterra",
        "hilltopads",
        "clickadu",
        "monetag",
        "adnxs.com",
        "taboola.com",
        "outbrain.com",
        "mgid.com",
        "revcontent.com",
        "acscdn.com",
        "juicyads",
        "tsyndicate",
        "realsrv",
        "onclickalgo",
    ];
    BLOCKED_PATTERNS.iter().any(|p| lower.contains(p))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DirectoryDownloadInterceptPayload {
    url: String,
    file_name: String,
    site_title: String,
}

#[allow(clippy::too_many_arguments)]
fn spawn_intercepted_directory_download(
    app: AppHandle,
    url: String,
    referer: Option<String>,
    cookie: Option<String>,
    user_agent: String,
    suggested_filename: Option<String>,
    save_dir: Option<String>,
    speed_limit: Option<f64>,
    mark_pre_resolved: bool,
    site_title: String,
) {
    tauri::async_runtime::spawn(async move {
        let display_name = derive_filename(&url, suggested_filename.as_deref(), None);
        if enqueue_download_internal(
            app.clone(),
            url.clone(),
            referer,
            cookie,
            Some(user_agent),
            suggested_filename,
            save_dir,
            Some(8),
            speed_limit,
            mark_pre_resolved,
            false,
        )
        .await
        .is_ok()
        {
            let _ = app.emit(
                "directory-download-intercepted",
                DirectoryDownloadInterceptPayload {
                    url,
                    file_name: display_name,
                    site_title,
                },
            );
        }
    });
}

#[tauri::command]
async fn open_directory_browser(
    app: AppHandle,
    url: String,
    title: Option<String>,
    custom_save_dir: Option<String>,
    max_speed_mbps: Option<f64>,
) -> Result<(), String> {
    let unwrapped_url = parse_duke_protocol_download_url(&url).unwrap_or(url);
    let parsed_url = validate_external_http_url(&unwrapped_url)?;
    let display_title = title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Game Directory Browser")
        .to_string();
    let window_title = format!("{} — Duke Safe Browser", display_title);

    if let Some(existing) = app.get_webview_window("directory-browser") {
        let _ = existing.set_title(&window_title);
        let _ = existing.navigate(parsed_url);
        let _ = existing.unminimize();
        let _ = existing.show();
        let _ = existing.set_focus();
        return Ok(());
    }

    let profile_dir = get_grabber_webview_profile_dir(&app)?;
    let initial_url_str = parsed_url.to_string();
    let last_page_url = Arc::new(Mutex::new(initial_url_str));

    let app_for_nav = app.clone();
    let save_dir_nav = custom_save_dir.clone();
    let speed_nav = max_speed_mbps;
    let last_page_nav = Arc::clone(&last_page_url);
    let site_title_nav = display_title.clone();

    let app_for_dl = app.clone();
    let save_dir_dl = custom_save_dir;
    let speed_dl = max_speed_mbps;
    let last_page_dl = Arc::clone(&last_page_url);
    let site_title_dl = display_title;

    let app_for_new = app.clone();

    let window = WebviewWindowBuilder::new(
        &app,
        "directory-browser",
        WebviewUrl::External(parsed_url),
    )
    .title(&window_title)
    .inner_size(1200.0, 800.0)
    .min_inner_size(820.0, 560.0)
    .center()
    .data_directory(profile_dir)
    .user_agent(WIN11_EDGE_USER_AGENT)
    .on_new_window(move |target_url, _features| {
        let target_str = target_url.as_str();
        if !is_ad_or_tracker_url(target_str) {
            if let Some(win) = app_for_new.get_webview_window("directory-browser") {
                let _ = win.navigate(target_url);
            }
        }
        NewWindowResponse::Deny
    })
    .on_download(move |webview, event| match event {
        DownloadEvent::Requested { url, destination } => {
            let dl_url = url.to_string();
            if is_ad_or_tracker_url(&dl_url) {
                return false;
            }
            let suggested = destination
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string());
            let referer = webview
                .url()
                .ok()
                .map(|u| u.to_string())
                .filter(|s| s.starts_with("http"))
                .or_else(|| {
                    last_page_dl
                        .lock()
                        .ok()
                        .map(|g| g.clone())
                        .filter(|s| !s.is_empty())
                });
            let cookie = collect_cookies_from_webview(&webview, &url, referer.as_deref());
            spawn_intercepted_directory_download(
                app_for_dl.clone(),
                dl_url,
                referer,
                (!cookie.is_empty()).then_some(cookie),
                WIN11_EDGE_USER_AGENT.to_string(),
                suggested,
                save_dir_dl.clone(),
                speed_dl,
                true,
                site_title_dl.clone(),
            );
            let _ = webview.window().hide();
            schedule_delayed_window_close(app_for_dl.clone(), "directory-browser");
            false
        }
        DownloadEvent::Finished { .. } => false,
        _ => true,
    })
    .on_navigation(move |nav_url: &Url| -> bool {
        let nav_str = nav_url.as_str();
        if is_ad_or_tracker_url(nav_str) {
            return false;
        }

        if is_direct_cdn_binary_url(nav_str) {
            let referer = last_page_nav
                .lock()
                .ok()
                .map(|g| g.clone())
                .filter(|s| !s.is_empty());
            let cookie = app_for_nav
                .get_webview_window("directory-browser")
                .map(|win| collect_cookies_from_window(&win, nav_url, referer.as_deref()))
                .unwrap_or_default();
            spawn_intercepted_directory_download(
                app_for_nav.clone(),
                nav_str.to_string(),
                referer,
                (!cookie.is_empty()).then_some(cookie),
                WIN11_EDGE_USER_AGENT.to_string(),
                None,
                save_dir_nav.clone(),
                speed_nav,
                true,
                site_title_nav.clone(),
            );
            if let Some(win) = app_for_nav.get_webview_window("directory-browser") {
                let _ = win.hide();
            }
            schedule_delayed_window_close(app_for_nav.clone(), "directory-browser");
            return false;
        }

        if let Ok(mut guard) = last_page_nav.lock() {
            *guard = nav_str.to_string();
        }
        true
    })
    .build()
    .map_err(|e| format!("Failed to open Duke Safe Browser: {e}"))?;

    window.on_window_event(|event| {
        if matches!(event, WindowEvent::Destroyed) {
            trim_process_working_set();
        }
    });

    let _ = window.set_focus();
    Ok(())
}

async fn resolve_stream_info(app: &AppHandle, raw_url: &str) -> Result<ResolvedStreamInfo, String> {
    let parsed_initial = validate_external_http_url(raw_url)?;
    let current_url = parsed_initial.to_string();

    if is_direct_cdn_binary_url(&current_url) {
        return Ok(ResolvedStreamInfo {
            stream_url: current_url.clone(),
            referer: current_url,
            cookie: String::new(),
            user_agent: get_session_user_agent().to_string(),
            suggested_filename: None,
        });
    }

    if let Some(native_resolved) = try_resolve_native_api(&current_url).await {
        return Ok(native_resolved);
    }

    capture_stream_via_webview(app, &current_url).await
}

fn aimd_next_chunk_bytes(
    current_chunk_bytes: u64,
    elapsed_secs: f64,
    bytes_transferred: u64,
    stalled_or_jitter: bool,
) -> u64 {
    let clamped = current_chunk_bytes.clamp(AIMD_MIN_CHUNK_BYTES, AIMD_MAX_CHUNK_BYTES);
    if stalled_or_jitter {
        // TCP-style Multiplicative Decrease on socket stall, timeout, or bandwidth jitter.
        return (clamped / 2).max(AIMD_MIN_CHUNK_BYTES);
    }
    if elapsed_secs > 0.10 && bytes_transferred >= AIMD_MIN_CHUNK_BYTES / 2 {
        let mbps = (bytes_transferred as f64 / (1024.0 * 1024.0)) / elapsed_secs;
        if mbps >= 4.0 {
            // TCP-style Additive Increase (+4 MiB) up to 64 MiB max slice on healthy throughput.
            return clamped.saturating_add(AIMD_STEP_BYTES).min(AIMD_MAX_CHUNK_BYTES);
        }
    }
    clamped
}

fn seek_write_all_at(file: &File, mut offset: u64, mut data: &[u8]) -> std::io::Result<()> {
    #[cfg(windows)]
    use std::os::windows::fs::FileExt;
    #[cfg(unix)]
    use std::os::unix::fs::FileExt;

    while !data.is_empty() {
        #[cfg(windows)]
        let written = file.seek_write(data, offset)?;
        #[cfg(unix)]
        let written = file.write_at(data, offset)?;
        if written == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "seek_write returned 0 bytes",
            ));
        }
        offset += written as u64;
        data = &data[written..];
    }
    Ok(())
}

struct AtomicSegmentSlot {
    start: AtomicU64,
    current: AtomicU64,
    end: AtomicU64,
    index: AtomicU64,
}

impl AtomicSegmentSlot {
    fn new(start: u64, current: u64, end: u64, index: u64) -> Self {
        Self {
            start: AtomicU64::new(start),
            current: AtomicU64::new(current),
            end: AtomicU64::new(end),
            index: AtomicU64::new(index),
        }
    }
}

const MAX_DYNAMIC_SEGMENTS: usize = 64;

fn try_steal_slowest_tail(
    slots: &[AtomicSegmentSlot],
    active_cnt: &AtomicU64,
    steal_lock: &Mutex<()>,
) -> Option<usize> {
    let _steal_guard = steal_lock.lock().unwrap_or_else(|e| e.into_inner());
    let active_count = (active_cnt.load(Ordering::Acquire) as usize).min(slots.len());
    if active_count >= slots.len() {
        return None;
    }

    let mut best_victim: Option<(usize, u64, u64, u64)> = None;
    for (cand, slot) in slots.iter().enumerate().take(active_count) {
        let c_curr = slot.current.load(Ordering::Acquire);
        let c_end = slot.end.load(Ordering::Acquire);
        if c_end != u64::MAX && c_curr < c_end {
            let rem = c_end.saturating_add(1).saturating_sub(c_curr);
            if rem >= WORK_STEAL_MIN_REMAINING_BYTES
                && best_victim.map(|(_, _, _, b_rem)| rem > b_rem).unwrap_or(true)
            {
                best_victim = Some((cand, c_curr, c_end, rem));
            }
        }
    }

    let (victim_slot, v_curr, v_end, rem) = best_victim?;
    let half_rem = (rem / 2) & !4095;
    let min_offset = AIMD_MIN_CHUNK_BYTES.min(rem / 2);
    let split_start = (v_curr + half_rem).max(v_curr + min_offset);
    if split_start <= v_curr || split_start > v_end {
        return None;
    }

    slots[victim_slot]
        .end
        .store(split_start - 1, Ordering::Release);
    let new_slot = active_count;
    let new_file_idx = (0..active_count)
        .map(|k| slots[k].index.load(Ordering::Relaxed) as usize + 1)
        .max()
        .unwrap_or(new_slot)
        .max(new_slot);

    slots[new_slot].start.store(split_start, Ordering::Release);
    slots[new_slot]
        .current
        .store(split_start, Ordering::Release);
    slots[new_slot].end.store(v_end, Ordering::Release);
    slots[new_slot]
        .index
        .store(new_file_idx as u64, Ordering::Release);
    active_cnt.store((new_slot + 1) as u64, Ordering::Release);
    Some(new_slot)
}

async fn execute_resumable_matrix_job(
    app: AppHandle,
    mut job: DownloadJob,
) -> Result<DownloadJob, String> {
    let pause_flag = Arc::new(AtomicBool::new(false));
    {
        let mut map = get_pause_map().lock().unwrap_or_else(|e| e.into_inner());
        map.insert(job.download_id.clone(), Arc::clone(&pause_flag));
    }

    // Keep job.resolved_url and job.stream_url synchronized
    if job.resolved_url.is_empty() && !job.stream_url.is_empty() {
        job.resolved_url = job.stream_url.clone();
    } else if job.stream_url.is_empty() && !job.resolved_url.is_empty() {
        job.stream_url = job.resolved_url.clone();
    }

    // Smart Stream Re-Validation before Token Refresh:
    let mut validated_probe: Option<reqwest::Response> = None;
    if !job.resolved_url.is_empty() {
        validated_probe = probe_direct_stream(
            &job.resolved_url,
            &job.referer,
            &job.cookie,
            &job.user_agent,
        )
        .await;
    }

    if validated_probe.is_none() && is_direct_cdn_binary_url(&job.original_url) {
        if let Some(direct_resp) = probe_direct_stream(
            &job.original_url,
            &job.original_url,
            &job.cookie,
            &job.user_agent,
        )
        .await
        {
            job.stream_url = direct_resp.url().to_string();
            job.resolved_url = job.stream_url.clone();
            if job.referer.is_empty() {
                job.referer = job.original_url.clone();
            }
            validated_probe = Some(direct_resp);
        }
    }

    let probe_resp = match validated_probe {
        Some(valid_resp) => valid_resp,
        None => {
            job.status = "RESOLVING".to_string();
            job.message = if job.downloaded_bytes > 0 {
                "Refreshing expired download token via Pure-Rust DoH Resolver...".to_string()
            } else {
                "Resolving direct CDN stream via Pure-Rust DoH Resolver...".to_string()
            };
            update_job_in_memory_and_disk(&app, &job, true);
            let _ = app.emit("download-progress", &job);

            match resolve_stream_info(&app, &job.original_url).await {
                Ok(info) => {
                    if pause_flag.load(Ordering::Relaxed) {
                        job.status = "PAUSED".to_string();
                        job.message = "Paused - Click RESUME to start".to_string();
                        update_job_in_memory_and_disk(&app, &job, true);
                        let _ = app.emit("download-progress", &job);
                        get_pause_map()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&job.download_id);
                        return Ok(job);
                    }

                    job.stream_url = info.stream_url.clone();
                    job.resolved_url = info.stream_url;
                    job.referer = info.referer;
                    job.cookie = info.cookie;
                    job.user_agent = info.user_agent;
                    if let Some(suggested) = info.suggested_filename {
                        if job.downloaded_bytes == 0 && job.segments.is_empty() {
                            let clean_name =
                                derive_filename(&job.stream_url, None, Some(&suggested));
                            let dir = PathBuf::from(&job.save_dir);
                            job.file_name = clean_name.clone();
                            job.final_path = dir.join(&clean_name).to_string_lossy().to_string();
                            job.part_path = dir
                                .join(format!("{}.part", clean_name))
                                .to_string_lossy()
                                .to_string();
                        }
                    }
                    update_job_in_memory_and_disk(&app, &job, true);

                    match probe_direct_stream(
                        &job.resolved_url,
                        &job.referer,
                        &job.cookie,
                        &job.user_agent,
                    )
                    .await
                    {
                        Some(r2) => r2,
                        None => {
                            job.status = "PAUSED".to_string();
                            job.message =
                                "CDN stream probe paused - Click RESUME to retry".to_string();
                            update_job_in_memory_and_disk(&app, &job, true);
                            let _ = app.emit("download-progress", &job);
                            get_pause_map()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&job.download_id);
                            return Err(job.message);
                        }
                    }
                }
                Err(e) => {
                    job.status = "PAUSED".to_string();
                    job.message = format!("Paused ({}) - Click RESUME to continue", e);
                    update_job_in_memory_and_disk(&app, &job, true);
                    let _ = app.emit("download-progress", &job);
                    get_pause_map()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&job.download_id);
                    return Err(e);
                }
            }
        }
    };

    let final_cdn_url = probe_resp.url().to_string();
    if final_cdn_url.starts_with("http") {
        job.stream_url = final_cdn_url.clone();
        job.resolved_url = final_cdn_url;
    }

    let is_partial = probe_resp.status().as_u16() == 206;
    let accept_ranges = probe_resp
        .headers()
        .get(ACCEPT_RANGES)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .contains("bytes");

    let probed_total = if is_partial {
        probe_resp
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.rsplit('/').next())
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(0)
    } else {
        probe_resp
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(0)
    };

    if job.total_bytes == 0 && probed_total > 0 {
        job.total_bytes = probed_total;
    }

    if job.downloaded_bytes == 0 && job.segments.is_empty() {
        let disp = probe_resp
            .headers()
            .get(CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok());
        let refined = derive_filename(&job.stream_url, disp, Some(&job.file_name));
        if refined != job.file_name {
            let dir = PathBuf::from(&job.save_dir);
            job.file_name = refined.clone();
            job.final_path = dir.join(&refined).to_string_lossy().to_string();
            job.part_path = dir
                .join(format!("{}.part", refined))
                .to_string_lossy()
                .to_string();
        }
    }

    let probe_content_type = probe_resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    drop(probe_resp);

    let can_multi = (is_partial || accept_ranges) && job.total_bytes > AIMD_MIN_CHUNK_BYTES * 2;
    let desired_threads = if can_multi {
        job.thread_count.clamp(1, 8)
    } else {
        1
    };

    if job.segments.is_empty() {
        if can_multi && desired_threads > 1 {
            let chunk_size = job.total_bytes / desired_threads as u64;
            for i in 0..desired_threads {
                let start_b = i as u64 * chunk_size;
                let end_b = if i == desired_threads - 1 {
                    job.total_bytes - 1
                } else {
                    (i as u64 + 1) * chunk_size - 1
                };
                job.segments.push(SegmentChunk {
                    index: i,
                    start_byte: start_b,
                    current_byte: start_b,
                    end_byte: end_b,
                    completed: false,
                });
            }
        } else {
            job.segments.push(SegmentChunk {
                index: 0,
                start_byte: 0,
                current_byte: 0,
                end_byte: if job.total_bytes > 0 {
                    job.total_bytes - 1
                } else {
                    u64::MAX
                },
                completed: false,
            });
        }
    }

    // Pre-allocate the single `.part` target file for lock-free positional `seek_write` I/O.
    let part_path_obj = PathBuf::from(&job.part_path);
    if let Some(parent_dir) = part_path_obj.parent() {
        let _ = fs::create_dir_all(parent_dir);
    }
    let part_existed = part_path_obj.exists();
    if !part_existed {
        for i in 0..MAX_DYNAMIC_SEGMENTS {
            let _ = fs::remove_file(segment_file_path(&job.part_path, i));
        }
        for seg in &mut job.segments {
            seg.current_byte = seg.start_byte;
            seg.completed = false;
        }
        job.downloaded_bytes = 0;
    } else {
        reconcile_job_bytes_from_disk(&mut job);
    }

    let stream_clean_title = clean_game_title(&job.file_name);
    let stream_extract_dir =
        PathBuf::from(&job.save_dir).join(sanitize_filename(&stream_clean_title));
    let stream_archive_fmt = stream_extract::detect_stream_archive_format(
        &job.file_name,
        probe_content_type.as_deref(),
        None,
    );
    let use_in_flight_zst = matches!(
        stream_archive_fmt,
        stream_extract::StreamArchiveFormat::TarZst
            | stream_extract::StreamArchiveFormat::ZstSingleFile
    ) && !part_existed
        && job.downloaded_bytes == 0;

    // For `.tar.zst` / `.zst` in-flight streaming over multiple HTTP connections, issue rolling 2 MiB
    // sliding windows so workers maintain full speed while bounding out-of-order reorder memory <= 24 MiB.
    const ZST_WINDOW_BYTES: u64 = 2 * 1024 * 1024;
    let zst_window_cursor: Option<Arc<AtomicU64>> = if use_in_flight_zst && can_multi && job.total_bytes > 0 {
        job.segments.clear();
        let mut cursor = 0u64;
        for idx in 0..desired_threads {
            if cursor >= job.total_bytes {
                break;
            }
            let end_b = (cursor + ZST_WINDOW_BYTES - 1).min(job.total_bytes - 1);
            job.segments.push(SegmentChunk {
                index: idx,
                start_byte: cursor,
                current_byte: cursor,
                end_byte: end_b,
                completed: false,
            });
            cursor = end_b.saturating_add(1);
        }
        Some(Arc::new(AtomicU64::new(cursor)))
    } else {
        None
    };
    let zst_total_bytes = job.total_bytes;
    let streamed_bytes_atomic = Arc::new(AtomicU64::new(0));

    let in_flight_zst_pipe: Arc<Mutex<Option<stream_extract::InFlightZstPipeline>>> =
        if use_in_flight_zst {
            let stripped_name = job
                .file_name
                .trim_end_matches(".zst")
                .trim_end_matches(".tzst");
            let single_target = PathBuf::from(&job.save_dir).join(stripped_name);
            Arc::new(Mutex::new(Some(
                stream_extract::InFlightZstPipeline::start(
                    stream_archive_fmt,
                    stream_extract_dir.clone(),
                    single_target,
                ),
            )))
        } else {
            Arc::new(Mutex::new(None))
        };

    let prealloc_total = job.total_bytes;
    let part_path_open = part_path_obj.clone();
    let shared_part_file: Option<Arc<File>> = if use_in_flight_zst {
        None
    } else {
        Some(Arc::new(
            tokio::task::spawn_blocking(move || -> Result<File, String> {
                let file = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(&part_path_open)
                    .map_err(|e| format!("Failed to open pre-allocated .part file: {}", e))?;
                if prealloc_total > 0 {
                    let current_len = file.metadata().map(|m| m.len()).unwrap_or(0);
                    if current_len != prealloc_total {
                        file.set_len(prealloc_total)
                            .map_err(|e| format!("Failed to pre-allocate target file: {}", e))?;
                    }
                }
                Ok(file)
            })
            .await
            .map_err(|e| format!("Pre-allocation task error: {}", e))??,
        ))
    };

    // Lock-Free AIMD Work-Stealing Engine:
    // Pre-allocate 64 atomic range slots synced into the pre-allocated `.part` file via `seek_write`.
    let initial_segments = job.segments.len().min(MAX_DYNAMIC_SEGMENTS);
    let active_seg_count = Arc::new(AtomicU64::new(initial_segments as u64));
    let work_steal_lock = Arc::new(Mutex::new(()));

    let mut slots_vec = Vec::with_capacity(MAX_DYNAMIC_SEGMENTS);
    for i in 0..MAX_DYNAMIC_SEGMENTS {
        if i < initial_segments {
            let s = &job.segments[i];
            slots_vec.push(AtomicSegmentSlot::new(
                s.start_byte,
                s.current_byte,
                s.end_byte,
                s.index as u64,
            ));
        } else {
            slots_vec.push(AtomicSegmentSlot::new(0, 0, 0, i as u64));
        }
    }
    let slots = Arc::new(slots_vec);
    let window_bytes_atomic = Arc::new(AtomicU64::new(0));

    let prior_elapsed_secs = job.elapsed_secs;
    let session_start = Instant::now();
    let is_finished = Arc::new(AtomicBool::new(false));

    job.status = "DOWNLOADING".to_string();
    job.message = format!(
        "Downloading ({} / {})",
        format_bytes(job.downloaded_bytes),
        format_bytes(job.total_bytes)
    );
    update_job_in_memory_and_disk(&app, &job, true);
    let _ = app.emit("download-progress", &job);

    let prio_coord = projfs_stream::register_job_coordinator(&job.download_id, job.total_bytes);
    let prio_coord_rep = Arc::clone(&prio_coord);

    let app_rep = app.clone();
    let job_rep_base = job.clone();
    let slots_rep = Arc::clone(&slots);
    let active_cnt_rep = Arc::clone(&active_seg_count);
    let win_bytes_rep = Arc::clone(&window_bytes_atomic);
    let streamed_rep = Arc::clone(&streamed_bytes_atomic);
    let is_fin_rep = Arc::clone(&is_finished);

    // 250ms EWMA telemetry aggregator (`max 4 IPC updates/sec`).
    let reporter_handle = tokio::spawn(async move {
        let mut job_state = job_rep_base;
        let mut last_tick = Instant::now();
        let mut last_disk_persist = Instant::now();
        let mut ema_speed_mbps: f64 = 0.0;

        while !is_fin_rep.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if is_fin_rep.load(Ordering::Relaxed) {
                break;
            }

            let count = (active_cnt_rep.load(Ordering::Acquire) as usize).min(MAX_DYNAMIC_SEGMENTS);
            let mut updated_segments = Vec::with_capacity(count);
            let mut total_downloaded = 0u64;

            for idx in 0..count {
                let s_byte = slots_rep[idx].start.load(Ordering::Acquire);
                let e_byte = slots_rep[idx].end.load(Ordering::Acquire);
                let f_idx = slots_rep[idx].index.load(Ordering::Relaxed) as usize;
                let curr_raw = slots_rep[idx].current.load(Ordering::Acquire);
                let curr = if e_byte != u64::MAX {
                    curr_raw.min(e_byte.saturating_add(1))
                } else {
                    curr_raw
                };
                let completed = e_byte != u64::MAX && curr > e_byte;
                total_downloaded += curr.saturating_sub(s_byte);
                updated_segments.push(SegmentChunk {
                    index: f_idx,
                    start_byte: s_byte,
                    current_byte: curr,
                    end_byte: e_byte,
                    completed,
                });
            }
            if use_in_flight_zst {
                total_downloaded = streamed_rep.load(Ordering::Relaxed);
            } else {
                job_state.segments = updated_segments;
            }

            let dt = last_tick.elapsed().as_secs_f64().max(0.05);
            let delta_bytes = win_bytes_rep.swap(0, Ordering::Relaxed);
            let instant_speed = (delta_bytes as f64 / (1024.0 * 1024.0)) / dt;

            if ema_speed_mbps <= 0.01 {
                ema_speed_mbps = instant_speed;
            } else {
                ema_speed_mbps = (ema_speed_mbps * 0.65) + (instant_speed * 0.35);
            }

            if ema_speed_mbps > job_state.peak_speed_mbps {
                job_state.peak_speed_mbps = ema_speed_mbps;
            }

            last_tick = Instant::now();

            let total_elapsed = prior_elapsed_secs + session_start.elapsed().as_secs();
            let eta_secs = if job_state.total_bytes > total_downloaded && ema_speed_mbps > 0.05 {
                let rem_mb =
                    (job_state.total_bytes - total_downloaded) as f64 / (1024.0 * 1024.0);
                Some((rem_mb / ema_speed_mbps) as u64)
            } else {
                None
            };

            let pct = if job_state.total_bytes > 0 {
                ((total_downloaded as f64 / job_state.total_bytes as f64) * 100.0).min(99.9)
            } else {
                50.0
            };

            job_state.downloaded_bytes = total_downloaded;
            job_state.speed_mbps = ema_speed_mbps;
            job_state.progress_pct = pct;
            job_state.elapsed_secs = total_elapsed;
            job_state.eta_secs = eta_secs;
            job_state.status = "DOWNLOADING".to_string();
            job_state.message = format!(
                "{} / {} ({}-Worker AIMD Work-Stealing DoH)",
                format_bytes(total_downloaded),
                if job_state.total_bytes > 0 {
                    format_bytes(job_state.total_bytes)
                } else {
                    "Stream".to_string()
                },
                initial_segments
            );

            let write_disk = last_disk_persist.elapsed() >= Duration::from_secs(3);
            if write_disk {
                last_disk_persist = Instant::now();
            }
            if prio_coord_rep.update_progress(total_downloaded) {
                let _ = app_rep.emit(
                    "projfs-stream-ready",
                    serde_json::json!({
                        "downloadId": job_state.download_id,
                        "fileName": job_state.file_name,
                        "saveDir": job_state.save_dir,
                        "progressPct": pct
                    }),
                );
            }
            update_job_in_memory_and_disk(&app_rep, &job_state, write_disk);
            let _ = app_rep.emit("download-progress", &job_state);
        }
    });

    // Build a DoH-resolved burst client for the CDN stream host
    let doh_burst_client = build_doh_client_for_url(&job.resolved_url, true, true).await;
    let zip_remote_plan: Option<Arc<stream_extract::ZipRemotePlan>> = if !use_in_flight_zst
        && stream_archive_fmt == stream_extract::StreamArchiveFormat::Zip
        && can_multi
        && job.total_bytes > 22
    {
        stream_extract::prefetch_zip_remote_plan(
            &doh_burst_client,
            &job.resolved_url,
            job.total_bytes,
            &job.referer,
            &job.cookie,
        )
            .await
            .map(Arc::new)
    } else {
        None
    };
    let extracted_zip_entries = Arc::new(Mutex::new(HashSet::<String>::new()));
    let zip_dispatch_lock = Arc::new(Mutex::new(()));
    let mut handles = Vec::with_capacity(initial_segments);

    for idx in 0..initial_segments {
        let seg = &job.segments[idx];
        if seg.completed && initial_segments == 1 {
            continue;
        }

        let url_clone = job.resolved_url.clone();
        let ref_clone = job.referer.clone();
        let cookie_clone = job.cookie.clone();
        let ua_clone = if job.user_agent.trim().is_empty() {
            get_session_user_agent().to_string()
        } else {
            job.user_agent.clone()
        };
        let slots_w = Arc::clone(&slots);
        let active_cnt_w = Arc::clone(&active_seg_count);
        let steal_lock_w = Arc::clone(&work_steal_lock);
        let win_bytes_worker = Arc::clone(&window_bytes_atomic);
        let streamed_worker = Arc::clone(&streamed_bytes_atomic);
        let zst_cursor_w = zst_window_cursor.clone();
        let pause_clone = Arc::clone(&pause_flag);
        let part_file_w = shared_part_file.clone();
        let in_flight_pipe_w = Arc::clone(&in_flight_zst_pipe);
        let zip_plan_w = zip_remote_plan.clone();
        let extracted_zip_w = Arc::clone(&extracted_zip_entries);
        let zip_dispatch_w = Arc::clone(&zip_dispatch_lock);
        let part_path_zip_w = part_path_obj.clone();
        let extract_dir_w = stream_extract_dir.clone();
        let prio_coord_w = Arc::clone(&prio_coord);
        let mut burst_client = doh_burst_client.clone();
        let total_segments_count = initial_segments as u64;

        handles.push(tokio::spawn(async move {
            if idx > 0 {
                tokio::time::sleep(Duration::from_millis(55 * idx as u64)).await;
            }

            let mut current_slot = idx;
            let mut consecutive_failures = 0u32;
            let mut adaptive_window_bytes = AIMD_MIN_CHUNK_BYTES;
            let mut staging_buf: Vec<u8> = Vec::with_capacity(ASYNC_SEGMENT_BUFFER_BYTES);

            loop {
                if pause_clone.load(Ordering::Relaxed) {
                    return Ok::<(), String>(());
                }

                // Demand-paged ProjFS byte ranges jump the queue across all 8 AIMD workers.
                let urgent_range = prio_coord_w.pop_urgent_range();
                let is_urgent = urgent_range.is_some();
                let (start_pos, seg_end_byte) = if let Some(ref urg) = urgent_range {
                    (urg.start, urg.end)
                } else {
                    (
                        slots_w[current_slot].current.load(Ordering::Acquire),
                        slots_w[current_slot].end.load(Ordering::Acquire),
                    )
                };

                if !is_urgent && seg_end_byte != u64::MAX && start_pos > seg_end_byte {
                    if let Some(ref zcursor) = zst_cursor_w {
                        loop {
                            if pause_clone.load(Ordering::Relaxed) {
                                return Ok::<(), String>(());
                            }
                            let pipe_pos = in_flight_pipe_w
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .as_ref()
                                .map(|p| p.next_contiguous_offset())
                                .unwrap_or(0);
                            if zcursor.load(Ordering::Acquire).saturating_sub(pipe_pos) < 24 * 1024 * 1024 {
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(15)).await;
                        }
                        let next_s = zcursor.fetch_add(ZST_WINDOW_BYTES, Ordering::AcqRel);
                        if next_s >= zst_total_bytes {
                            return Ok(());
                        }
                        let next_e = (next_s + ZST_WINDOW_BYTES - 1).min(zst_total_bytes - 1);
                        slots_w[current_slot].start.store(next_s, Ordering::Release);
                        slots_w[current_slot].current.store(next_s, Ordering::Release);
                        slots_w[current_slot].end.store(next_e, Ordering::Release);
                        consecutive_failures = 0;
                        adaptive_window_bytes = AIMD_MIN_CHUNK_BYTES;
                        continue;
                    }
                    if let Some(new_slot) =
                        try_steal_slowest_tail(&slots_w, &active_cnt_w, &steal_lock_w)
                    {
                        current_slot = new_slot;
                        consecutive_failures = 0;
                        adaptive_window_bytes = AIMD_MIN_CHUNK_BYTES;
                        continue;
                    }
                    return Ok(());
                }

                let burst_end = if seg_end_byte != u64::MAX {
                    start_pos
                        .saturating_add(adaptive_window_bytes)
                        .saturating_sub(1)
                        .min(seg_end_byte)
                } else {
                    u64::MAX
                };

                let mut req = burst_client
                    .get(&url_clone)
                    .header(USER_AGENT, &ua_clone)
                    .header(REFERER, &ref_clone)
                    .header("Connection", "close");

                if !cookie_clone.is_empty() {
                    req = req.header(COOKIE, &cookie_clone);
                }

                if burst_end != u64::MAX {
                    req = req.header(RANGE, format!("bytes={}-{}", start_pos, burst_end));
                } else if start_pos > 0 {
                    req = req.header(RANGE, format!("bytes={}-", start_pos));
                }

                let mut resp = match tokio::time::timeout(Duration::from_secs(8), req.send()).await
                {
                    Ok(Ok(r))
                        if r.status().as_u16() == 206
                            || (r.status().is_success() && start_pos == 0) =>
                    {
                        r
                    }
                    Ok(Ok(r)) => {
                        consecutive_failures += 1;
                        adaptive_window_bytes =
                            aimd_next_chunk_bytes(adaptive_window_bytes, 0.0, 0, true);
                        rotate_doh_cached_ips(&url_clone);
                        burst_client = build_doh_client_for_url(&url_clone, true, true).await;
                        if consecutive_failures == 4 {
                            let _ = probe_direct_stream(
                                &url_clone,
                                &ref_clone,
                                &cookie_clone,
                                &ua_clone,
                            )
                            .await;
                        }
                        if consecutive_failures > 25 {
                            return Err(format!("Segment {} HTTP {}", current_slot, r.status()));
                        }
                        tokio::time::sleep(Duration::from_millis(600)).await;
                        continue;
                    }
                    _ => {
                        consecutive_failures += 1;
                        adaptive_window_bytes =
                            aimd_next_chunk_bytes(adaptive_window_bytes, 0.0, 0, true);
                        rotate_doh_cached_ips(&url_clone);
                        burst_client = build_doh_client_for_url(&url_clone, true, true).await;
                        if consecutive_failures == 4 {
                            let _ = probe_direct_stream(
                                &url_clone,
                                &ref_clone,
                                &cookie_clone,
                                &ua_clone,
                            )
                            .await;
                        }
                        if consecutive_failures > 25 {
                            return Err(format!("Segment {} connection timed out", current_slot));
                        }
                        tokio::time::sleep(Duration::from_millis(600)).await;
                        continue;
                    }
                };

                let mut flush_offset = start_pos;
                let mut burst_bytes = 0u64;
                let burst_start = Instant::now();
                let mut throttled_or_recycled = false;
                let mut stalled_or_jitter = false;
                staging_buf.clear();

                let flush_staged =
                    |buf: &mut Vec<u8>,
                     offset: &mut u64,
                     slot: usize,
                     slots_ref: &[AtomicSegmentSlot]|
                     -> Result<u64, String> {
                        if buf.is_empty() {
                            return Ok(0);
                        }
                        let live_end = if is_urgent {
                            seg_end_byte
                        } else {
                            slots_ref[slot].end.load(Ordering::Acquire)
                        };
                        let allowed = if live_end != u64::MAX {
                            buf.len().min(
                                live_end.saturating_add(1).saturating_sub(*offset) as usize,
                            )
                        } else {
                            buf.len()
                        };
                        if allowed > 0 {
                            if let Some(ref pf) = part_file_w {
                                seek_write_all_at(pf, *offset, &buf[..allowed])
                                    .map_err(|e| format!("Positional disk write error: {}", e))?;
                            } else if let Some(ref pipe) = *in_flight_pipe_w.lock().unwrap_or_else(|e| e.into_inner()) {
                                pipe.submit_range(*offset, &buf[..allowed])?;
                                streamed_worker.fetch_add(allowed as u64, Ordering::Relaxed);
                            }
                            let range_start = *offset;
                            *offset += allowed as u64;
                            if !is_urgent {
                                slots_ref[slot].current.store(*offset, Ordering::Release);
                            }
                            prio_coord_w.mark_range_completed(
                                range_start,
                                offset.saturating_sub(1),
                                is_urgent,
                            );
                            win_bytes_worker.fetch_add(allowed as u64, Ordering::Relaxed);
                        }
                        buf.clear();
                        Ok(allowed as u64)
                    };

                loop {
                    if pause_clone.load(Ordering::Relaxed) {
                        let _ = flush_staged(
                            &mut staging_buf,
                            &mut flush_offset,
                            current_slot,
                            &slots_w,
                        )?;
                        return Ok(());
                    }

                    // Recycle any socket stalling for > 4 seconds
                    let next_chunk =
                        tokio::time::timeout(Duration::from_secs(4), resp.chunk()).await;

                    match next_chunk {
                        Ok(Ok(Some(chunk))) => {
                            consecutive_failures = 0;
                            let live_end = if is_urgent {
                                seg_end_byte
                            } else {
                                slots_w[current_slot].end.load(Ordering::Acquire)
                            };
                            let effective_end = if burst_end != u64::MAX && live_end != u64::MAX {
                                burst_end.min(live_end)
                            } else if live_end != u64::MAX {
                                live_end
                            } else {
                                burst_end
                            };

                            let cursor_pos = flush_offset + staging_buf.len() as u64;
                            let mut accept_len = chunk.len() as u64;
                            if effective_end != u64::MAX {
                                let rem = effective_end.saturating_add(1).saturating_sub(cursor_pos);
                                accept_len = accept_len.min(rem);
                            }

                            if accept_len > 0 {
                                let slice = &chunk[..accept_len as usize];
                                for sub in slice.chunks(REUSABLE_STREAM_CHUNK_BYTES) {
                                    if staging_buf.len() + sub.len() > ASYNC_SEGMENT_BUFFER_BYTES {
                                        burst_bytes += flush_staged(
                                            &mut staging_buf,
                                            &mut flush_offset,
                                            current_slot,
                                            &slots_w,
                                        )?;
                                    }
                                    staging_buf.extend_from_slice(sub);
                                }
                            }

                            if staging_buf.len() >= ASYNC_SEGMENT_BUFFER_BYTES {
                                burst_bytes += flush_staged(
                                    &mut staging_buf,
                                    &mut flush_offset,
                                    current_slot,
                                    &slots_w,
                                )?;
                            }

                            let after_cursor = flush_offset + staging_buf.len() as u64;
                            if effective_end != u64::MAX && after_cursor > effective_end {
                                break;
                            }

                            // AIMD congestion & jitter guard:
                            let elapsed_secs = burst_start.elapsed().as_secs_f64();
                            if elapsed_secs > 6.0 {
                                let transferred_so_far = after_cursor.saturating_sub(start_pos);
                                let avg_kbps = (transferred_so_far as f64 / 1024.0) / elapsed_secs;
                                if avg_kbps < 300.0 {
                                    throttled_or_recycled = true;
                                    stalled_or_jitter = true;
                                    rotate_doh_cached_ips(&url_clone);
                                    break;
                                }
                            }

                            let limit_kbps = GLOBAL_SPEED_LIMIT_KBPS.load(Ordering::Relaxed);
                            if limit_kbps > 0 {
                                throttled_or_recycled = true;
                                let per_seg_kbps =
                                    (limit_kbps / total_segments_count.max(1)).max(128) as f64;
                                let elapsed = burst_start.elapsed().as_secs_f64().max(0.01);
                                let transferred_so_far = after_cursor.saturating_sub(start_pos);
                                let target_elapsed =
                                    (transferred_so_far as f64 / 1024.0) / per_seg_kbps;
                                if target_elapsed > elapsed {
                                    let sleep_ms =
                                        ((target_elapsed - elapsed) * 1000.0).min(100.0) as u64;
                                    if sleep_ms > 2 {
                                        tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
                                    }
                                }
                            }
                        }
                        Ok(Ok(None)) => {
                            let after_cursor = flush_offset + staging_buf.len() as u64;
                            if burst_end != u64::MAX && after_cursor <= burst_end {
                                stalled_or_jitter = true;
                            }
                            break;
                        }
                        _ => {
                            throttled_or_recycled = true;
                            stalled_or_jitter = true;
                            rotate_doh_cached_ips(&url_clone);
                            break;
                        }
                    }
                }

                burst_bytes += flush_staged(
                    &mut staging_buf,
                    &mut flush_offset,
                    current_slot,
                    &slots_w,
                )?;

                if let Some(ref zplan) = zip_plan_w {
                    let cnt =
                        (active_cnt_w.load(Ordering::Acquire) as usize).min(MAX_DYNAMIC_SEGMENTS);
                    let mut ranges = Vec::with_capacity(cnt);
                    for s_i in 0..cnt {
                        let sb = slots_w[s_i].start.load(Ordering::Acquire);
                        let cur = slots_w[s_i].current.load(Ordering::Acquire);
                        let eb = slots_w[s_i].end.load(Ordering::Acquire);
                        let covered_end = if eb != u64::MAX {
                            cur.min(eb.saturating_add(1))
                        } else {
                            cur
                        };
                        if covered_end > sb {
                            ranges.push((sb, covered_end));
                        }
                    }
                    ranges.sort_unstable();
                    let mut merged: Vec<(u64, u64)> = Vec::new();
                    for (s, e) in ranges {
                        if let Some(last) = merged.last_mut() {
                            if s <= last.1 {
                                last.1 = last.1.max(e);
                                continue;
                            }
                        }
                        merged.push((s, e));
                    }
                    let zplan_c = Arc::clone(zplan);
                    let extracted_c = Arc::clone(&extracted_zip_w);
                    let dispatch_c = Arc::clone(&zip_dispatch_w);
                    let part_c = part_path_zip_w.clone();
                    let dir_c = extract_dir_w.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        if let Ok(_guard) = dispatch_c.try_lock() {
                            let mut set_guard = extracted_c.lock().unwrap_or_else(|e| e.into_inner());
                            let _ = stream_extract::dispatch_ready_zip_entries(
                                &part_c,
                                &dir_c,
                                &zplan_c,
                                &merged,
                                &mut set_guard,
                            );
                        }
                    })
                    .await;
                }

                if throttled_or_recycled || stalled_or_jitter {
                    burst_client = build_doh_client_for_url(&url_clone, true, true).await;
                }

                // AIMD adaptive chunk scaling (4 MiB initial -> +4 MiB additive increase up to 64 MiB;
                // multiplicative half-reduction on stall/jitter).
                let slice_elapsed = burst_start.elapsed().as_secs_f64();
                adaptive_window_bytes = aimd_next_chunk_bytes(
                    adaptive_window_bytes,
                    slice_elapsed,
                    burst_bytes,
                    stalled_or_jitter,
                );
            }
        }));
    }

    let mut worker_error: Option<String> = None;
    for h in handles {
        match h.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                worker_error = Some(e);
            }
            Err(e) => {
                worker_error = Some(format!("Worker panic: {}", e));
            }
        }
    }

    is_finished.store(true, Ordering::Relaxed);
    let _ = reporter_handle.await;
    projfs_stream::unregister_job_coordinator(&job.download_id);
    get_pause_map()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&job.download_id);

    // Sync and drop the pre-allocated `.part` file handle before finalizing or pausing.
    if let Some(part_file_sync) = shared_part_file {
        let _ = tokio::task::spawn_blocking(move || {
            let _ = part_file_sync.sync_all();
        })
        .await;
    }

    // Synchronize final segment table (including stolen spans) sorted by start_byte.
    let final_seg_count = (active_seg_count.load(Ordering::Acquire) as usize).min(MAX_DYNAMIC_SEGMENTS);
    let mut final_segments = Vec::with_capacity(final_seg_count);
    for idx in 0..final_seg_count {
        let s_byte = slots[idx].start.load(Ordering::Acquire);
        let e_byte = slots[idx].end.load(Ordering::Acquire);
        let f_idx = slots[idx].index.load(Ordering::Relaxed) as usize;
        let curr = slots[idx].current.load(Ordering::Acquire);
        final_segments.push(SegmentChunk {
            index: f_idx,
            start_byte: s_byte,
            current_byte: curr,
            end_byte: e_byte,
            completed: e_byte != u64::MAX && curr > e_byte,
        });
    }
    final_segments.sort_by_key(|s| s.start_byte);
    job.segments = final_segments;

    if !use_in_flight_zst {
        reconcile_job_bytes_from_disk(&mut job);
    } else {
        job.downloaded_bytes = job
            .segments
            .iter()
            .map(|s| s.current_byte.saturating_sub(s.start_byte))
            .sum();
    }
    job.elapsed_secs = prior_elapsed_secs + session_start.elapsed().as_secs();
    job.speed_mbps = 0.0;
    job.eta_secs = None;

    if pause_flag.load(Ordering::Relaxed) {
        let _ = in_flight_zst_pipe
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        job.status = "PAUSED".to_string();
        job.message = format!(
            "Paused at {} - Saved to disk (Click RESUME anytime)",
            format_bytes(job.downloaded_bytes)
        );
        update_job_in_memory_and_disk(&app, &job, true);
        let _ = app.emit("download-progress", &job);
        trim_process_working_set();
        return Ok(job);
    }

    if let Some(err_msg) = worker_error {
        let _ = in_flight_zst_pipe
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        job.status = "PAUSED".to_string();
        job.message = format!(
            "Saved at {} ({}) - Click RESUME to continue",
            format_bytes(job.downloaded_bytes),
            err_msg
        );
        update_job_in_memory_and_disk(&app, &job, true);
        let _ = app.emit("download-progress", &job);
        trim_process_working_set();
        return Ok(job);
    }

    let zst_pipe_opt = {
        in_flight_zst_pipe
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    };
    if let Some(pipe) = zst_pipe_opt {
        job.status = "SCANNING".to_string();
        job.message = "Finalizing in-flight zstd stream & running Security Shield...".to_string();
        update_job_in_memory_and_disk(&app, &job, true);
        let _ = app.emit("download-progress", &job);

        let stream_stats = tokio::task::spawn_blocking(move || pipe.finish())
            .await
            .map_err(|e| format!("In-flight zstd finalize task failed: {}", e))??;

        let dir_check = stream_extract_dir.clone();
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            if dir_check.is_dir() {
                validate_extracted_tree_against_zip_slip(&dir_check)?;
                unblock_ntfs_zone_identifier(&dir_check);
            }
            Ok(())
        })
        .await
        .map_err(|e| format!("Stream validation failed: {}", e))??;

        let final_size = stream_stats.extracted_bytes.max(job.downloaded_bytes);
        job.downloaded_bytes = stream_stats.compressed_bytes_streamed.max(job.downloaded_bytes);
        job.total_bytes = job.downloaded_bytes;
        job.progress_pct = 100.0;

        let detected_exe = find_best_game_exe_in_folder(&stream_extract_dir, &stream_clean_title);
        let mut scan_verdict = "CLEAN".to_string();
        let mut scan_summary = format!(
            "In-flight extracted {} files ({}) directly to disk.",
            stream_stats.files_extracted,
            format_bytes(final_size)
        );
        if let Some(ref exe) = detected_exe {
            if let Ok(sr) = scan_file_security_core(exe.to_string_lossy().into_owned()).await {
                scan_verdict = sr.verdict;
                scan_summary = sr.summary;
            }
        }
        job.verdict = Some(scan_verdict.clone());
        let mut should_auto_clean = false;

        if scan_verdict != "DANGER" {
            if let Some(exe) = detected_exe {
                if let Ok(added) = add_local_game_with_origin(
                    app.clone(),
                    exe.to_string_lossy().into_owned(),
                    Some(stream_clean_title.clone()),
                    None,
                    Some("Ready".to_string()),
                    None,
                    Some("turbo_downloader"),
                )
                .await
                {
                    if let Ok(lib) = load_library_from_disk(&app) {
                        let _ = app.emit("library-updated", &lib);
                    }
                    job.status = "COMPLETED".to_string();
                    job.registered_in_library = true;
                    should_auto_clean = true;
                    job.message = format!(
                        "Stream-extracted & installed '{}' ({}) in flight!",
                        added.title,
                        format_bytes(final_size)
                    );
                } else {
                    job.status = "COMPLETED".to_string();
                    job.message = scan_summary;
                }
            } else {
                job.status = "COMPLETED".to_string();
                job.message = scan_summary;
            }
        } else {
            job.status = "COMPLETED".to_string();
            job.registered_in_library = false;
            job.message = format!("BLOCKED ({}) - {}", format_bytes(final_size), scan_summary);
        }

        update_job_in_memory_and_disk(&app, &job, true);
        let _ = app.emit("download-progress", &job);
        if should_auto_clean {
            schedule_auto_clean_completed_job(app.clone(), job.download_id.clone());
        }
        trim_process_working_set();
        return Ok(job);
    }

    job.status = "SCANNING".to_string();
    job.message = "Finalizing archive & running Entropy/PE Malware Shield...".to_string();
    update_job_in_memory_and_disk(&app, &job, true);
    let _ = app.emit("download-progress", &job);

    let final_dest = PathBuf::from(&job.final_path);
    if final_dest.exists() {
        let _ = fs::remove_file(&final_dest);
    }

    // Zero-copy atomic metadata rename from pre-allocated `.part` to final destination.
    if part_path_obj.exists() && fs::rename(&part_path_obj, &final_dest).is_err() {
        let p_clone = part_path_obj.clone();
        let f_clone = final_dest.clone();
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            fs::copy(&p_clone, &f_clone)
                .map_err(|e| format!("Failed to finalize download: {}", e))?;
            let _ = fs::remove_file(&p_clone);
            Ok(())
        })
        .await
        .map_err(|e| format!("Finalize task failed: {}", e))??;
    }
    for i in 0..MAX_DYNAMIC_SEGMENTS {
        let _ = fs::remove_file(segment_file_path(&job.part_path, i));
    }

    let final_size = fs::metadata(&final_dest)
        .map(|m| m.len())
        .unwrap_or(job.downloaded_bytes);
    if let Some(zplan) = zip_remote_plan {
        if final_dest.is_file() {
            let final_c = final_dest.clone();
            let dir_c = stream_extract_dir.clone();
            let extracted_c = Arc::clone(&extracted_zip_entries);
            let _ = tokio::task::spawn_blocking(move || {
                let mut set_guard = extracted_c.lock().unwrap_or_else(|e| e.into_inner());
                let _ = stream_extract::dispatch_ready_zip_entries(
                    &final_c,
                    &dir_c,
                    &zplan,
                    &[(0, final_size)],
                    &mut set_guard,
                );
                if dir_c.is_dir() {
                    unblock_ntfs_zone_identifier(&dir_c);
                }
            })
            .await;
        }
    }
    job.downloaded_bytes = final_size;
    job.total_bytes = final_size;
    job.progress_pct = 100.0;

    let scan_report = scan_file_security(job.final_path.clone()).await?;
    job.verdict = Some(scan_report.verdict.clone());

    let mut should_auto_clean = false;

    if scan_report.verdict != "DANGER" {
        job.status = "INSTALLING".to_string();
        job.message =
            "Clean file! Extracting archive & adding game to MY GAMES automatically...".to_string();
        update_job_in_memory_and_disk(&app, &job, true);
        let _ = app.emit("download-progress", &job);

        match auto_install_downloaded_file(&app, &job.final_path).await {
            Ok(added_game) => {
                job.status = "COMPLETED".to_string();
                job.registered_in_library = true;
                should_auto_clean = true;
                job.message = format!(
                    "Installed '{}' ({}) to MY GAMES!",
                    added_game.title, added_game.size_formatted
                );
            }
            Err(install_err) => {
                job.status = "COMPLETED".to_string();
                job.registered_in_library = false;
                job.message = format!(
                    "Completed ({}) - {}",
                    format_bytes(final_size),
                    install_err
                );
            }
        }
    } else {
        job.status = "COMPLETED".to_string();
        job.registered_in_library = false;
        job.message = format!("BLOCKED ({}) - {}", format_bytes(final_size), scan_report.summary);
    }

    update_job_in_memory_and_disk(&app, &job, true);
    let _ = app.emit("download-progress", &job);
    if should_auto_clean {
        schedule_auto_clean_completed_job(app.clone(), job.download_id.clone());
    }
    trim_process_working_set();

    Ok(job)
}

fn spawn_queue_scheduler(app: AppHandle) {
    if QUEUE_RUNNER_ACTIVE
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    tauri::async_runtime::spawn(async move {
        loop {
            let next_job_opt = {
                let guard = get_queue_store().lock().unwrap_or_else(|p| p.into_inner());
                let any_active = guard.iter().any(|j| {
                    j.status == "DOWNLOADING"
                        || j.status == "RESOLVING"
                        || j.status == "INSTALLING"
                });
                if any_active {
                    None
                } else {
                    guard.iter().find(|j| j.status == "QUEUED").cloned()
                }
            };

            match next_job_opt {
                Some(job) => {
                    let _ = execute_resumable_matrix_job(app.clone(), job).await;
                }
                None => break,
            }
        }
        QUEUE_RUNNER_ACTIVE.store(false, Ordering::SeqCst);
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn enqueue_download_internal(
    app: AppHandle,
    url: String,
    referer: Option<String>,
    cookie: Option<String>,
    user_agent: Option<String>,
    suggested_filename: Option<String>,
    custom_save_dir: Option<String>,
    threads: Option<usize>,
    max_speed_mbps: Option<f64>,
    mark_pre_resolved: bool,
    reset_speed_limit_when_none: bool,
) -> Result<Vec<DownloadJob>, String> {
    let unwrapped_url = parse_duke_protocol_download_url(&url).unwrap_or(url);
    let parsed_url = validate_external_http_url(&unwrapped_url)?;
    let trimmed = parsed_url.to_string();

    if let Some(speed) = max_speed_mbps {
        GLOBAL_SPEED_LIMIT_KBPS.store((speed * 1024.0) as u64, Ordering::Relaxed);
    } else if reset_speed_limit_when_none {
        let current_settings = load_settings_from_disk(&app);
        GLOBAL_SPEED_LIMIT_KBPS.store(effective_speed_limit_kbps(&current_settings), Ordering::Relaxed);
    }

    let download_dir = match custom_save_dir.filter(|d| !d.trim().is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => resolve_default_downloads_dir(&app),
    };

    if !download_dir.exists() {
        fs::create_dir_all(&download_dir)
            .map_err(|e| format!("Failed to create download folder: {}", e))?;
    }

    ensure_queue_initialized(&app);

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let initial_name = derive_filename(&trimmed, suggested_filename.as_deref(), None);
    let save_dir_str = download_dir.to_string_lossy().to_string();
    let final_path = download_dir.join(&initial_name);
    let part_path = download_dir.join(format!("{}.part", initial_name));
    let pre_resolved = if mark_pre_resolved || is_direct_cdn_binary_url(&trimmed) {
        trimmed.clone()
    } else {
        String::new()
    };

    let job = DownloadJob {
        download_id: format!("dl_{}", now_ms),
        original_url: trimmed,
        stream_url: pre_resolved.clone(),
        resolved_url: pre_resolved,
        referer: referer.unwrap_or_default(),
        cookie: cookie.unwrap_or_default(),
        user_agent: user_agent
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| rotate_privacy_user_agent().to_string()),
        file_name: initial_name,
        save_dir: save_dir_str,
        part_path: part_path.to_string_lossy().to_string(),
        final_path: final_path.to_string_lossy().to_string(),
        downloaded_bytes: 0,
        total_bytes: 0,
        speed_mbps: 0.0,
        peak_speed_mbps: 0.0,
        progress_pct: 0.0,
        elapsed_secs: 0,
        eta_secs: None,
        thread_count: threads.unwrap_or(8).clamp(1, 8),
        segments: Vec::new(),
        status: "QUEUED".to_string(),
        message: "Queued - Starting download...".to_string(),
        verdict: None,
        registered_in_library: false,
        added_at: now_ms / 1000,
    };

    update_job_in_memory_and_disk(&app, &job, true);
    let _ = app.emit("download-progress", &job);

    spawn_queue_scheduler(app.clone());

    let guard = get_queue_store().lock().unwrap_or_else(|e| e.into_inner());
    Ok(guard.clone())
}

#[tauri::command]
async fn enqueue_download(
    app: AppHandle,
    url: String,
    custom_save_dir: Option<String>,
    threads: Option<usize>,
    max_speed_mbps: Option<f64>,
) -> Result<Vec<DownloadJob>, String> {
    enqueue_download_internal(
        app,
        url,
        None,
        None,
        None,
        None,
        custom_save_dir,
        threads,
        max_speed_mbps,
        false,
        true,
    )
    .await
}

#[tauri::command]
async fn install_downloaded_job(app: AppHandle, download_id: String) -> Result<GameEntry, String> {
    ensure_queue_initialized(&app);
    let mut target_job = {
        let guard = get_queue_store().lock().unwrap_or_else(|e| e.into_inner());
        guard
            .iter()
            .find(|j| j.download_id == download_id)
            .cloned()
            .ok_or_else(|| "Download job not found.".to_string())?
    };

    target_job.status = "INSTALLING".to_string();
    target_job.message = "Extracting archive & adding game to MY GAMES...".to_string();
    update_job_in_memory_and_disk(&app, &target_job, true);
    let _ = app.emit("download-progress", &target_job);

    match auto_install_downloaded_file(&app, &target_job.final_path).await {
        Ok(added) => {
            target_job.status = "COMPLETED".to_string();
            target_job.registered_in_library = true;
            target_job.message = format!(
                "Installed '{}' ({}) to MY GAMES!",
                added.title, added.size_formatted
            );
            update_job_in_memory_and_disk(&app, &target_job, true);
            let _ = app.emit("download-progress", &target_job);
            schedule_auto_clean_completed_job(app.clone(), target_job.download_id.clone());
            Ok(added)
        }
        Err(e) => {
            target_job.status = "COMPLETED".to_string();
            target_job.registered_in_library = false;
            target_job.message = format!("Install note: {}", e);
            update_job_in_memory_and_disk(&app, &target_job, true);
            let _ = app.emit("download-progress", &target_job);
            Err(e)
        }
    }
}

#[tauri::command]
async fn clear_completed_downloads(app: AppHandle) -> Result<Vec<DownloadJob>, String> {
    ensure_queue_initialized(&app);
    let remaining = {
        let mut guard = get_queue_store().lock().unwrap_or_else(|e| e.into_inner());
        guard.retain(|j| j.status != "COMPLETED");
        persist_queue_to_disk(&app, &guard);
        guard.clone()
    };
    let _ = app.emit("download-queue-updated", &remaining);
    Ok(remaining)
}

#[tauri::command]
async fn get_launcher_settings(app: AppHandle) -> Result<LauncherSettings, String> {
    let app_clone = app.clone();
    tokio::task::spawn_blocking(move || load_settings_from_disk(&app_clone).to_ipc_safe())
        .await
        .map_err(|e| format!("Failed to read settings: {}", e))
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn update_launcher_settings(
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

static PENDING_PROTOCOL_URLS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

fn get_pending_protocol_urls() -> &'static Mutex<Vec<String>> {
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

fn focus_main_window(app: &AppHandle) {
    if let Some(daemon) = TRAY_DAEMON.get() {
        daemon.ui_visible.store(true, Ordering::Release);
    }
    duke_core::tray_daemon::reopen_main_window(app, "main");
}

fn enqueue_protocol_download_url(
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
mod win32_clipboard_protocol {
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
mod win32_clipboard_protocol {
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
fn read_clipboard_text() -> Result<String, String> {
    win32_clipboard_protocol::read_clipboard_text()
}

#[tauri::command]
fn write_clipboard_text(text: String) -> Result<(), String> {
    win32_clipboard_protocol::write_clipboard_text(&text)
}

#[tauri::command]
fn take_pending_protocol_urls() -> Vec<String> {
    let mut guard = get_pending_protocol_urls()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    std::mem::take(&mut *guard)
}

#[tauri::command]
async fn set_speed_limit(max_speed_mbps: Option<f64>) -> Result<(), String> {
    if let Some(speed) = max_speed_mbps {
        GLOBAL_SPEED_LIMIT_KBPS.store((speed * 1024.0) as u64, Ordering::Relaxed);
    } else {
        GLOBAL_SPEED_LIMIT_KBPS.store(0, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
async fn pause_download(app: AppHandle, download_id: String) -> Result<Vec<DownloadJob>, String> {
    ensure_queue_initialized(&app);
    if let Some(flag) = get_pause_map()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&download_id)
    {
        flag.store(true, Ordering::Relaxed);
    }

    let mut guard = get_queue_store().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(job) = guard.iter_mut().find(|j| j.download_id == download_id) {
        reconcile_job_bytes_from_disk(job);
        job.status = "PAUSED".to_string();
        job.speed_mbps = 0.0;
        job.eta_secs = None;
        job.message = format!(
            "Paused at {} - Saved to disk (Click RESUME anytime)",
            format_bytes(job.downloaded_bytes)
        );
    }
    persist_queue_to_disk(&app, &guard);
    let result = guard.clone();
    drop(guard);

    spawn_queue_scheduler(app);
    Ok(result)
}

#[tauri::command]
async fn resume_download(
    app: AppHandle,
    download_id: String,
    max_speed_mbps: Option<f64>,
) -> Result<Vec<DownloadJob>, String> {
    if let Some(speed) = max_speed_mbps {
        GLOBAL_SPEED_LIMIT_KBPS.store((speed * 1024.0) as u64, Ordering::Relaxed);
    } else {
        GLOBAL_SPEED_LIMIT_KBPS.store(0, Ordering::Relaxed);
    }

    ensure_queue_initialized(&app);
    let target_job = {
        let mut guard = get_queue_store().lock().unwrap_or_else(|e| e.into_inner());
        let any_running = guard.iter().any(|j| {
            j.download_id != download_id
                && (j.status == "DOWNLOADING"
                    || j.status == "RESOLVING"
                    || j.status == "INSTALLING")
        });

        if let Some(job) = guard.iter_mut().find(|j| j.download_id == download_id) {
            reconcile_job_bytes_from_disk(job);
            if any_running {
                job.status = "QUEUED".to_string();
                job.message =
                    "Queued - Will resume automatically when current download finishes".to_string();
                persist_queue_to_disk(&app, &guard);
                None
            } else {
                job.status = "DOWNLOADING".to_string();
                job.message = format!(
                    "Resuming from {}...",
                    format_bytes(job.downloaded_bytes)
                );
                let cloned = job.clone();
                persist_queue_to_disk(&app, &guard);
                Some(cloned)
            }
        } else {
            return Err("Download job not found.".to_string());
        }
    };

    if let Some(job_to_run) = target_job {
        let app_clone = app.clone();
        tokio::spawn(async move {
            let _ = execute_resumable_matrix_job(app_clone.clone(), job_to_run).await;
            spawn_queue_scheduler(app_clone);
        });
    } else {
        spawn_queue_scheduler(app.clone());
    }

    let guard = get_queue_store().lock().unwrap_or_else(|e| e.into_inner());
    Ok(guard.clone())
}

#[tauri::command]
async fn remove_download_job(
    app: AppHandle,
    download_id: String,
    delete_file: bool,
) -> Result<Vec<DownloadJob>, String> {
    ensure_queue_initialized(&app);
    if let Some(flag) = get_pause_map()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&download_id)
    {
        flag.store(true, Ordering::Relaxed);
    }

    let mut guard = get_queue_store().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(job) = guard.iter().find(|j| j.download_id == download_id) {
        if delete_file {
            for i in 0..32 {
                let _ = fs::remove_file(segment_file_path(&job.part_path, i));
            }
            let _ = fs::remove_file(&job.part_path);
            let _ = fs::remove_file(&job.final_path);
        }
    }
    guard.retain(|j| j.download_id != download_id);
    persist_queue_to_disk(&app, &guard);
    Ok(guard.clone())
}

#[tauri::command]
async fn get_download_jobs(
    app: AppHandle,
    backup_jobs: Option<Vec<DownloadJob>>,
) -> Result<Vec<DownloadJob>, String> {
    let mut current = ensure_queue_initialized(&app);

    if current.is_empty() {
        if let Some(backups) = backup_jobs {
            if !backups.is_empty() {
                let mut guard = get_queue_store().lock().unwrap_or_else(|e| e.into_inner());
                for mut b in backups {
                    reconcile_job_bytes_from_disk(&mut b);
                    if b.status == "DOWNLOADING" || b.status == "RESOLVING" {
                        b.status = "PAUSED".to_string();
                        b.speed_mbps = 0.0;
                        b.message = format!(
                            "Saved at {} - Click RESUME to continue",
                            format_bytes(b.downloaded_bytes)
                        );
                    }
                    guard.push(b);
                }
                persist_queue_to_disk(&app, &guard);
                current = guard.clone();
            }
        }
    }

    Ok(current)
}

#[tauri::command]
async fn auto_fix_all_covers(app: AppHandle) -> Result<Vec<GameEntry>, String> {
    let mut lib = tokio::task::spawn_blocking({
        let app_c = app.clone();
        move || load_library_from_disk(&app_c)
    })
    .await
    .map_err(|e| format!("Failed loading library: {e}"))??;

    let covers_dir = get_covers_dir(&app)?;
    let icons_dir = sidebar_icons::get_sidebar_icons_dir(&app)?;
    let api_key = load_settings_from_disk(&app)
        .steamgriddb_api_key
        .unwrap_or_default();
    let sem = Arc::new(Semaphore::new(4));
    let mut join_set = tokio::task::JoinSet::new();

    for entry in &lib {
        let covers_c = covers_dir.clone();
        let icons_c = icons_dir.clone();
        let sem_c = sem.clone();
        let key_c = api_key.clone();
        let entry_c = entry.clone();

        join_set.spawn(async move {
            let _permit = sem_c.acquire_owned().await.ok()?;
            let (inferred_pid, plat_hint) = artwork::infer_game_platform_hint(
                &entry_c.title,
                &entry_c.exe_path,
                &entry_c.genre,
                &entry_c.source,
            );
            let eff_pid = (entry_c.platform_id > 0)
                .then_some(entry_c.platform_id)
                .or(inferred_pid);
            let steam_aid = entry_c
                .app_id
                .or_else(|| entry_c.steam_app_id.and_then(|v| u32::try_from(v).ok()));

            // Verify existing local cover file or resolve via cascading 4-tier pipeline + 32x32 sidebar icon
            let req = artwork::CoverResolveRequest {
                cache_id: entry_c.id.clone(),
                title: entry_c.title.clone(),
                platform_id: eff_pid,
                platform_hint: plat_hint,
                steam_app_id: steam_aid,
                existing_cover_url: (!entry_c.cover_url.trim().is_empty())
                    .then(|| entry_c.cover_url.trim().to_string()),
                steamgriddb_api_key: key_c,
                sidebar_icon_id: Some(entry_c.id.clone()),
            };
            let outcome = artwork::resolve_and_cache_cover_in_dirs(&req, &covers_c, &icons_c).await;
            if let Some(ref out) = outcome {
                return Some((entry_c.id, out.cover_path.to_string_lossy().to_string()));
            }
            // Even when no remote cover image was found, still extract/generate 32x32 sidebar icon from .exe
            let src = sidebar_icons::SidebarIconSource::from(&entry_c);
            let _ = sidebar_icons::resolve_sidebar_icon_in_dirs(&src, &icons_c, &covers_c);
            None
        });
    }

    let mut updated_map: HashMap<String, String> = HashMap::new();
    while let Some(res) = join_set.join_next().await {
        if let Ok(Some((gid, cover_path))) = res {
            updated_map.insert(gid, cover_path);
        }
    }

    let mut changed = false;
    for entry in &mut lib {
        if let Some(new_cover) = updated_map.get(&entry.id) {
            if entry.cover_url != *new_cover {
                entry.cover_url = new_cover.clone();
                changed = true;
            }
        }
    }

    if changed {
        save_library_to_disk(&app, &lib)?;
    }
    let _ = app.emit("library-updated", &lib);
    let _ = app.emit("sidebar-icons-updated", ());
    trim_process_working_set();
    Ok(lib)
}

#[tauri::command]
async fn find_and_apply_cover(
    app: AppHandle,
    id: String,
    custom_query: Option<String>,
) -> Result<Vec<GameEntry>, String> {
    let mut library = load_library_from_disk(&app)?;
    let mut updated_id: Option<String> = None;
    let has_custom_query = custom_query
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    if has_custom_query {
        if let Ok(covers_dir) = get_covers_dir(&app) {
            artwork::clear_cached_cover_for_id(&covers_dir, &id);
        }
    }
    if let Some(entry) = library.iter_mut().find(|g| g.id == id) {
        let search_title = custom_query
            .filter(|q| !q.trim().is_empty())
            .unwrap_or_else(|| entry.title.clone());

        let (cover, app_id, matched_name) = smart_find_game_cover(
            &app,
            &entry.id,
            &search_title,
            &entry.exe_path,
            &entry.install_dir,
        )
        .await;

        if !cover.is_empty() {
            entry.cover_url = cover;
            entry.steam_app_id = app_id;
            entry.app_id = app_id.and_then(|sid| u32::try_from(sid).ok());
            if let Some(official) = matched_name {
                entry.title = official;
            }
            updated_id = Some(entry.id.clone());
        } else {
            trim_process_working_set();
            return Err(format!(
                "No cover found for '{}'. Try typing a different title.",
                search_title
            ));
        }
    }
    save_library_to_disk(&app, &library)?;
    if let Some(gid) = updated_id {
        sidebar_icons::invalidate_sidebar_icon(&app, &gid);
    }
    let _ = app.emit("library-updated", &library);
    let _ = app.emit("sidebar-icons-updated", ());
    trim_process_working_set();
    Ok(library)
}

#[tauri::command]
async fn scan_file_security(path: String) -> Result<SecurityScanReport, String> {
    scan_file_security_core(path).await
}

#[inline]
fn bytes_contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack.len() >= needle.len()
        && haystack.windows(needle.len()).any(|w| w == needle)
}

fn shannon_entropy_from_histogram(hist: &[u64; 256], total: u64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    let total_f = total as f64;
    let mut entropy = 0.0_f64;
    for &count in hist.iter() {
        if count > 0 {
            let p = count as f64 / total_f;
            entropy -= p * p.log2();
        }
    }
    entropy.clamp(0.0, 8.0)
}

#[derive(Debug, Clone, Default)]
struct PeHeaderSummary {
    architecture: String,
    kind: &'static str,
    subsystem: &'static str,
    section_count: u16,
    sections: Vec<String>,
    has_packer_section: bool,
}

fn inspect_pe_headers_from_file(file: &mut File, size_bytes: u64) -> Option<PeHeaderSummary> {
    if size_bytes < 64 {
        return None;
    }
    file.seek(SeekFrom::Start(0)).ok()?;
    let mut dos = [0u8; 64];
    file.read_exact(&mut dos).ok()?;
    if &dos[0..2] != b"MZ" {
        return None;
    }
    let e_lfanew = u32::from_le_bytes([dos[0x3C], dos[0x3D], dos[0x3E], dos[0x3F]]) as u64;
    if e_lfanew < 0x40 || e_lfanew.saturating_add(24) > size_bytes {
        return None;
    }

    file.seek(SeekFrom::Start(e_lfanew)).ok()?;
    let mut nt_buf = [0u8; 264];
    let nt_read = file.read(&mut nt_buf).ok()?;
    if nt_read < 24 || &nt_buf[0..4] != b"PE\0\0" {
        return None;
    }

    let machine = u16::from_le_bytes([nt_buf[4], nt_buf[5]]);
    let architecture = match machine {
        0x8664 => "x64 (AMD64)".to_string(),
        0x014C => "x86 (32-bit)".to_string(),
        0xAA64 => "ARM64".to_string(),
        other => format!("Machine 0x{:04X}", other),
    };
    let section_count = u16::from_le_bytes([nt_buf[6], nt_buf[7]]);
    let size_of_opt = u16::from_le_bytes([nt_buf[20], nt_buf[21]]);
    let characteristics = u16::from_le_bytes([nt_buf[22], nt_buf[23]]);
    let kind = if (characteristics & 0x2000) != 0 {
        "DLL"
    } else {
        "EXE"
    };

    let subsystem = if nt_read >= 24 + 70 && size_of_opt >= 70 {
        let sub_id = u16::from_le_bytes([nt_buf[24 + 68], nt_buf[24 + 69]]);
        match sub_id {
            2 => "Windows GUI",
            3 => "Windows CUI (Console)",
            1 => "Native",
            _ => "Other Subsystem",
        }
    } else {
        "Unknown Subsystem"
    };

    let section_table_offset = e_lfanew + 24 + size_of_opt as u64;
    let mut sections = Vec::with_capacity((section_count as usize).min(16));
    let mut has_packer_section = false;
    if section_table_offset < size_bytes && file.seek(SeekFrom::Start(section_table_offset)).is_ok()
    {
        let mut sec_hdr = [0u8; 40];
        for _ in 0..section_count.min(16) {
            if file.read_exact(&mut sec_hdr).is_err() {
                break;
            }
            let raw_name = &sec_hdr[0..8];
            let end = raw_name.iter().position(|&b| b == 0).unwrap_or(8);
            let name = String::from_utf8_lossy(&raw_name[..end]).trim().to_string();
            if !name.is_empty() {
                let lower = name.to_ascii_lowercase();
                if lower.contains("upx")
                    || lower.contains("themida")
                    || lower.contains("vmp")
                    || lower.contains("enigma")
                    || lower.contains("aspack")
                    || lower.contains("mpress")
                    || lower.contains("petite")
                {
                    has_packer_section = true;
                }
                sections.push(name);
            }
        }
    }

    Some(PeHeaderSummary {
        architecture,
        kind,
        subsystem,
        section_count,
        sections,
        has_packer_section,
    })
}

pub(crate) async fn scan_file_security_core(path: String) -> Result<SecurityScanReport, String> {
    let target = PathBuf::from(path.trim().trim_matches('"'));
    if !target.exists() {
        return Err(format!("File does not exist: {}", path));
    }

    tokio::task::spawn_blocking(move || -> Result<SecurityScanReport, String> {
        let metadata = fs::metadata(&target)
            .map_err(|e| format!("Failed to read file metadata: {}", e))?;
        let size_bytes = metadata.len();
        let size_formatted = format_bytes(size_bytes);
        let file_name = target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        let lower_name = file_name.to_lowercase();
        let lower_full_path = target.to_string_lossy().to_lowercase();

        let mut file = File::open(&target)
            .map_err(|e| format!("Failed to open file for security scan: {}", e))?;

        let mut header = [0u8; 64];
        let header_read = file.read(&mut header).unwrap_or(0);
        let pe_summary = if header_read >= 2 && &header[0..2] == b"MZ" {
            inspect_pe_headers_from_file(&mut file, size_bytes)
        } else {
            None
        };
        let _ = file.seek(SeekFrom::Start(0));

        // Crack-Aware Whitelist Signatures (zero-false-positive emulator & scene tags)
        const CRACK_WHITELIST_SIGNATURES: &[&str] = &[
            "steam_api.dll",
            "steam_api64.dll",
            "onlinefix64.dll",
            "custom.dll",
            "winmm.dll",
            "goldberg",
            "steamemu",
            "rune",
            "tenoke",
            "codex",
            "flt",
            "elamigos",
            "steamless",
        ];

        let mut matched_whitelist_tag: Option<String> = None;
        for &sig in CRACK_WHITELIST_SIGNATURES {
            if lower_name == sig || lower_full_path.contains(sig) {
                matched_whitelist_tag = Some(sig.to_uppercase());
                break;
            }
        }

        // Zero-Allocation Streaming 64 KB PE / Shannon-Entropy / Heuristic Scanner
        // Computes Sha256, 256-bin byte histogram -> Shannon entropy (0.0-8.0), and ASCII signatures in-place.
        const MAX_HASH_AND_WINDOW_BYTES: u64 = 64 * 1024 * 1024;
        const OVERLAP_CAPACITY: usize = 256;

        let mut hasher = Sha256::new();
        let mut scan_buf = vec![0u8; OVERLAP_CAPACITY + SCAN_WINDOW_BYTES];
        let mut overlap_len: usize = 0;
        let mut byte_histogram = [0u64; 256];
        let mut histogram_total: u64 = 0;
        let mut scanned_bytes: u64 = 0;

        let mut has_dpapi_unprotect = false;
        let mut has_http_exfil = false;
        let mut has_browser_or_wallet_theft = false;
        let mut has_encoded_ps_dropper = false;

        let inspect_slice_in_place = |buf: &mut [u8],
                                      ov_len: usize,
                                      n: usize,
                                      matched_whitelist_tag: &mut Option<String>,
                                      has_dpapi_unprotect: &mut bool,
                                      has_http_exfil: &mut bool,
                                      has_browser_or_wallet_theft: &mut bool,
                                      has_encoded_ps_dropper: &mut bool| {
            for b in &mut buf[ov_len..ov_len + n] {
                *b = if b.is_ascii_graphic() || *b == b' ' {
                    b.to_ascii_lowercase()
                } else {
                    b' '
                };
            }
            let view = &buf[..ov_len + n];
            if matched_whitelist_tag.is_none() {
                for &sig in CRACK_WHITELIST_SIGNATURES {
                    if bytes_contains_subslice(view, sig.as_bytes()) {
                        *matched_whitelist_tag = Some(sig.to_uppercase());
                        break;
                    }
                }
            }
            if bytes_contains_subslice(view, b"cryptunprotectdata") {
                *has_dpapi_unprotect = true;
            }
            if bytes_contains_subslice(view, b"winhttpsendrequest")
                || bytes_contains_subslice(view, b"internetopenurl")
                || bytes_contains_subslice(view, b"winhttpconnect")
            {
                *has_http_exfil = true;
            }
            if bytes_contains_subslice(view, b"login data")
                || bytes_contains_subslice(view, b"local state")
                || bytes_contains_subslice(view, b"wallet.dat")
                || bytes_contains_subslice(view, b"discord/local storage/leveldb")
            {
                *has_browser_or_wallet_theft = true;
            }
            if bytes_contains_subslice(view, b"powershell")
                && (bytes_contains_subslice(view, b" -enc ")
                    || bytes_contains_subslice(view, b" -encodedcommand")
                    || bytes_contains_subslice(view, b"frombase64string")
                    || bytes_contains_subslice(view, b"-windowstyle hidden"))
            {
                *has_encoded_ps_dropper = true;
            }
        };

        while scanned_bytes < MAX_HASH_AND_WINDOW_BYTES {
            let n = file
                .read(&mut scan_buf[overlap_len..overlap_len + SCAN_WINDOW_BYTES])
                .unwrap_or(0);
            if n == 0 {
                break;
            }
            for &b in &scan_buf[overlap_len..overlap_len + n] {
                byte_histogram[b as usize] += 1;
            }
            histogram_total += n as u64;
            hasher.update(&scan_buf[overlap_len..overlap_len + n]);

            inspect_slice_in_place(
                &mut scan_buf,
                overlap_len,
                n,
                &mut matched_whitelist_tag,
                &mut has_dpapi_unprotect,
                &mut has_http_exfil,
                &mut has_browser_or_wallet_theft,
                &mut has_encoded_ps_dropper,
            );

            let total_view = overlap_len + n;
            let keep = total_view.min(OVERLAP_CAPACITY);
            scan_buf.copy_within(total_view - keep..total_view, 0);
            overlap_len = keep;
            scanned_bytes += n as u64;
        }

        // Also inspect the final 64 KB tail of files larger than 64 MB using SeekFrom::End
        if size_bytes > MAX_HASH_AND_WINDOW_BYTES
            && size_bytes > SCAN_WINDOW_BYTES as u64
            && file
                .seek(SeekFrom::End(-(SCAN_WINDOW_BYTES as i64)))
                .is_ok()
        {
            let n = file.read(&mut scan_buf[..SCAN_WINDOW_BYTES]).unwrap_or(0);
            if n > 0 {
                for &b in &scan_buf[..n] {
                    byte_histogram[b as usize] += 1;
                }
                histogram_total += n as u64;
                inspect_slice_in_place(
                    &mut scan_buf,
                    0,
                    n,
                    &mut matched_whitelist_tag,
                    &mut has_dpapi_unprotect,
                    &mut has_http_exfil,
                    &mut has_browser_or_wallet_theft,
                    &mut has_encoded_ps_dropper,
                );
            }
        }

        let shannon_entropy = shannon_entropy_from_histogram(&byte_histogram, histogram_total);
        let sha256_hex = hex::encode(hasher.finalize());
        let header_ascii_lower = String::from_utf8_lossy(&header[..header_read]).to_lowercase();

        let detected_format = if header_read >= 6 && &header[0..6] == b"Rar!\x1A\x07" {
            "RAR Archive"
        } else if header_read >= 6 && header[0..6] == [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C] {
            "7-Zip Archive"
        } else if header_read >= 4 && &header[0..4] == b"PK\x03\x04" {
            "ZIP Archive"
        } else if header_read >= 2 && header[0..2] == [0x1F, 0x8B] {
            "GZIP / TAR.GZ Archive"
        } else if header_read >= 2 && &header[0..2] == b"MZ" {
            "Windows PE Executable (.exe/.dll)"
        } else if header_ascii_lower.starts_with("<!do")
            || header_ascii_lower.starts_with("<htm")
            || header_ascii_lower.starts_with("<script")
            || header_ascii_lower.contains("<html")
        {
            "HTML / Script Payload"
        } else {
            "Game Archive / Binary"
        };

        let entropy_label = if shannon_entropy >= 7.20 {
            "High (Packed / Encrypted / Compressed)"
        } else if shannon_entropy >= 5.80 {
            "Moderate-High (Dense Binary / Assets)"
        } else if histogram_total == 0 {
            "Empty"
        } else {
            "Normal / Structured"
        };

        let mut details = vec![
            format!("Type: {}", detected_format),
            format!("SHA-256: {}...", &sha256_hex[..24]),
            format!("Shannon Entropy: {:.3} bits/byte ({})", shannon_entropy, entropy_label),
        ];

        let mut pe_architecture = None;
        let mut pe_sections = Vec::new();
        if let Some(ref pe) = pe_summary {
            pe_architecture = Some(format!("{} {}", pe.architecture, pe.kind));
            pe_sections = pe.sections.clone();
            let sec_preview = if pe.sections.is_empty() {
                "none".to_string()
            } else {
                pe.sections.join(", ")
            };
            details.push(format!(
                "PE Headers: {} {} | {} | {} sections [{}]",
                pe.architecture, pe.kind, pe.subsystem, pe.section_count, sec_preview
            ));
        }

        let mut verdict = "SAFE".to_string();
        let mut summary = "Clean file. Ready to extract and play.".to_string();

        // Check Crack-Aware Whitelist first for legitimate offline emulators & scene wrappers
        if let Some(ref tag) = matched_whitelist_tag {
            verdict = "CLEAN (VERIFIED GAME EMULATOR)".to_string();
            summary = format!(
                "Verified legitimate offline game emulator / scene release ({}). Whitelisted with zero false positives.",
                tag
            );
            details.push(format!("Crack-Aware Whitelist: {} verified", tag));
        }

        // High-Precision Malware Detection:
        // 1. Archive disguise attacks (.rar/.7z/.zip starting with MZ 4D 5A or HTML/script headers)
        let claims_archive = is_supported_archive_name(&lower_name);
        if claims_archive && detected_format.starts_with("Windows PE Executable") {
            verdict = "DANGER".to_string();
            summary = "ARCHIVE DISGUISE ATTACK: This .rar/.7z/.zip file starts with a Windows PE ('MZ' 4D 5A) executable header.".to_string();
            details.push("Heuristic: Disguised PE executable inside archive extension".to_string());
        } else if detected_format == "HTML / Script Payload" {
            verdict = "DANGER".to_string();
            summary = "Downloaded file is an HTML/Script web page rather than a valid game binary or archive.".to_string();
        }

        // 2. Double extensions & script droppers (.rar.exe, .scr, .hta, .vbs, .cmd, .bat, .ps1)
        let dangerous_exts = [
            ".rar.exe", ".zip.exe", ".7z.exe", ".iso.exe", ".scr", ".hta", ".vbs", ".pif", ".cmd",
            ".bat", ".ps1",
        ];
        if dangerous_exts.iter().any(|ext| lower_name.ends_with(ext)) {
            verdict = "DANGER".to_string();
            summary = "MALWARE ALERT: Double-extension or script dropper executable detected.".to_string();
            details.push("Heuristic: High-risk dropper extension blocked".to_string());
        }

        // 3. Small standalone .exe binaries (<15 MB) with infostealer / dropper signatures or packed high-entropy sections
        let is_small_exe = lower_name.ends_with(".exe") && size_bytes < 15 * 1024 * 1024;
        let has_combined_infostealer_sigs =
            has_dpapi_unprotect && has_http_exfil && has_browser_or_wallet_theft;
        let has_packed_pe = pe_summary
            .as_ref()
            .map(|p| p.has_packer_section || shannon_entropy >= 7.20)
            .unwrap_or(false);

        if is_small_exe && (has_combined_infostealer_sigs || has_encoded_ps_dropper) {
            verdict = "DANGER".to_string();
            summary = "INFOSTEALER / DROPPER DETECTED: Small executable contains credential/token exfiltration or encoded PowerShell payload signatures.".to_string();
            details.push("Heuristic: Combined DPAPI + HTTP exfil + credential store or -enc dropper".to_string());
        } else if is_small_exe && has_packed_pe && matched_whitelist_tag.is_none() {
            verdict = "DANGER".to_string();
            summary = format!(
                "PACKED DROPPER ALERT ({}): Small unverified executable shows high Shannon entropy ({:.2} b/B) or obfuscated PE sections.",
                size_formatted, shannon_entropy
            );
            details.push("Heuristic: Packed/encrypted PE payload under 15 MB".to_string());
        } else if is_small_exe && matched_whitelist_tag.is_none() {
            verdict = "DANGER".to_string();
            summary = format!(
                "FAKE DOWNLOAD ALERT ({}): Standalone unverified .exe under 15 MB flagged as potential dropper.",
                size_formatted
            );
        }

        let prog_files =
            std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string());
        let defender_path = PathBuf::from(prog_files)
            .join("Windows Defender")
            .join("MpCmdRun.exe");

        if defender_path.exists() && verdict != "DANGER" {
            let mut cmd = Command::new(defender_path);
            cmd.creation_flags(CREATE_NO_WINDOW);
            if let Ok(output) = cmd
                .args([
                    "-Scan",
                    "-ScanType",
                    "3",
                    "-File",
                    &target.to_string_lossy(),
                    "-DisableRemediation",
                ])
                .output()
            {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stdout_lower = stdout.to_lowercase();

                if stdout_lower.contains("threat") {
                    let is_real_malware = stdout_lower.contains("trojan")
                        || stdout_lower.contains("stealer")
                        || stdout_lower.contains("ransom")
                        || stdout_lower.contains("backdoor")
                        || stdout_lower.contains("lumma")
                        || stdout_lower.contains("redline")
                        || stdout_lower.contains("vidar");

                    let is_game_false_positive = stdout_lower.contains("hacktool")
                        || stdout_lower.contains("gamehack")
                        || stdout_lower.contains("pua:")
                        || stdout_lower.contains("crack")
                        || matched_whitelist_tag.is_some();

                    if is_real_malware && matched_whitelist_tag.is_none() {
                        verdict = "DANGER".to_string();
                        summary =
                            "MALWARE DETECTED: Windows Defender identified a Trojan/Infostealer signature."
                                .to_string();
                    } else if is_game_false_positive {
                        if matched_whitelist_tag.is_some() {
                            verdict = "CLEAN (VERIFIED GAME EMULATOR)".to_string();
                        } else {
                            verdict = "FALSE_POSITIVE_WARNING".to_string();
                        }
                        summary =
                            "CLEAN (VERIFIED GAME EMULATOR): Ignored harmless Steam wrapper / scene false-positive alert."
                                .to_string();
                    }
                }
            }
        }

        // 4. Security Shield v2: Static PE32/PE32+ Import APIs + Droppers + WinVerifyTrust Authenticode + Quarantine
        let mut shield_res = pe_inspect::PeSecurityShieldResult {
            shield_badge: if verdict == "DANGER" {
                "Blocked".to_string()
            } else {
                "Safe".to_string()
            },
            authenticode_status: if pe_summary.is_some() {
                "Unsigned".to_string()
            } else {
                "NotApplicable".to_string()
            },
            authenticode_detail: String::new(),
            imported_api_hits: Vec::new(),
            suspicious_behaviors: Vec::new(),
            quarantine_path: None,
        };

        let is_pe_target = pe_summary.is_some()
            || lower_name.ends_with(".exe")
            || lower_name.ends_with(".dll")
            || lower_name.ends_with(".asi");
        if is_pe_target {
            if let Ok(mut pe_shield) = pe_inspect::inspect_pe_security_shield_file(
                &target,
                matched_whitelist_tag.is_some(),
                Some(shannon_entropy),
            ) {
                details.push(format!(
                    "Authenticode: {} ({})",
                    pe_shield.authenticode_status, pe_shield.authenticode_detail
                ));
                if !pe_shield.imported_api_hits.is_empty() {
                    details.push(format!(
                        "Security-Relevant Imports: {}",
                        pe_shield.imported_api_hits.join(", ")
                    ));
                }
                for f in &pe_shield.suspicious_behaviors {
                    details.push(format!("[{}] {}: {}", f.severity, f.title, f.explanation));
                }
                if pe_shield.shield_badge == "Blocked" {
                    verdict = "DANGER".to_string();
                    summary = pe_shield
                        .suspicious_behaviors
                        .first()
                        .map(|f| format!("BLOCKED ({}): {}", f.title, f.explanation))
                        .unwrap_or_else(|| {
                            "Security Shield v2 blocked this executable due to high-risk behaviors.".to_string()
                        });
                    if !cfg!(test) {
                        if let Ok(q_dir) = pe_inspect::quarantine_root_dir_standalone() {
                            let reasons: Vec<String> = pe_shield
                                .suspicious_behaviors
                                .iter()
                                .map(|f| format!("{}: {}", f.title, f.explanation))
                                .collect();
                            if let Ok(q_path) = pe_inspect::quarantine_high_risk_binary(
                                &target,
                                &q_dir,
                                &reasons,
                                Some(&sha256_hex),
                            ) {
                                let q_str = q_path.to_string_lossy().to_string();
                                details.push(format!("Quarantined automatically to: {}", q_str));
                                pe_shield.quarantine_path = Some(q_str);
                            }
                        }
                    }
                } else if pe_shield.shield_badge == "Warning" && verdict == "SAFE" {
                    summary = format!(
                        "WARNING: {} Caution advised before launching outside AppContainer Jail.",
                        pe_shield
                            .suspicious_behaviors
                            .first()
                            .map(|f| f.explanation.as_str())
                            .unwrap_or("Review executable heuristics or enable AppContainer Jail.")
                    );
                }
                if verdict == "DANGER" && pe_shield.shield_badge != "Blocked" {
                    // Keep Security Shield v2 PE heuristics distinct from small-stub packed checks
                }
                shield_res = pe_shield;
            }
        }

        // Automatic NTFS Mark-of-the-Web (Zone.Identifier) Unblocker ONLY after passing security inspection
        let mut zone_unblocked = false;
        if verdict != "DANGER" && shield_res.shield_badge != "Blocked" {
            unblock_ntfs_zone_identifier(&target);
            zone_unblocked = true;
            details.push("NTFS Zone.Identifier: Unblocked after passing Security Shield v2 inspection".to_string());
        }

        Ok(SecurityScanReport {
            file_path: target.to_string_lossy().to_string(),
            file_name,
            size_bytes,
            size_formatted,
            sha256: sha256_hex,
            detected_format: detected_format.to_string(),
            verdict,
            summary,
            details,
            shannon_entropy,
            pe_architecture,
            pe_sections,
            shield_badge: shield_res.shield_badge,
            authenticode_status: shield_res.authenticode_status,
            authenticode_detail: shield_res.authenticode_detail,
            imported_api_hits: shield_res.imported_api_hits,
            suspicious_behaviors: shield_res.suspicious_behaviors,
            quarantine_path: shield_res.quarantine_path,
            zone_unblocked,
        })
    })
    .await
    .map_err(|e| format!("Security scan task failed: {}", e))?
}

#[tauri::command]
async fn delete_flagged_file(path: String) -> Result<(), String> {
    let target = PathBuf::from(path.trim().trim_matches('"'));
    if target.exists() && target.is_file() {
        fs::remove_file(&target).map_err(|e| format!("Failed to delete file: {}", e))?;
    }
    Ok(())
}

#[tauri::command]
async fn unblock_motw_for_path(path: String) -> Result<usize, String> {
    let clean = path.trim().trim_matches('"').to_string();
    if clean.is_empty() {
        return Err("Target path is empty.".to_string());
    }
    let target = PathBuf::from(&clean);
    if !target.exists() {
        return Err(format!("Target path does not exist: {}", clean));
    }
    tokio::task::spawn_blocking(move || Ok(unblock_ntfs_zone_identifier_raw(&target)))
        .await
        .map_err(|e| format!("MOTW unblock task failed: {}", e))?
}

#[tauri::command]
async fn get_directory_size(path: String) -> Result<u64, String> {
    let target = PathBuf::from(&path);
    if !target.exists() {
        return Err(format!("Target path does not exist: {}", path));
    }

    let resolved_dir = resolve_game_root_dir(&target);
    tokio::task::spawn_blocking(move || calculate_dir_size_sync(&resolved_dir))
        .await
        .map_err(|e| format!("Directory size task failed: {}", e))
}

/// Resolves retail price first via `0 ms` memory-mapped `catalog.dkcat`, falling back to Steam store search/details over `net-doh`.
async fn resolve_game_retail_price_fast(
    app_id_hint: Option<u32>,
    title: &str,
) -> (Option<u32>, u32, String) {
    let mut resolved_aid = app_id_hint.filter(|&id| id > 0);
    if let Some(entry) = catalog_mmap::lookup_entry_by_title_or_appid_via_mmap(resolved_aid, title) {
        if resolved_aid.is_none() && entry.app_id > 0 {
            resolved_aid = Some(entry.app_id);
        }
        let retail_cents = entry.initial_price_cents.max(entry.price_cents);
        if retail_cents > 0 {
            let formatted = format!("${:.2}", retail_cents as f64 / 100.0);
            return (resolved_aid, retail_cents, formatted);
        }
    }

    if resolved_aid.is_none() {
        if let Some((aid, _)) = resolve_steam_appid_by_title(title).await {
            resolved_aid = Some(aid);
        }
    }
    if let Some(aid) = resolved_aid {
        let (cents, fmt) = fetch_steam_app_retail_price(aid).await;
        return (Some(aid), cents, fmt);
    }
    (None, 0, "$0.00".to_string())
}

pub fn compute_money_saved_cents(library: &[GameEntry]) -> u64 {
    library
        .iter()
        .filter(|g| g.acquisition_source == "turbo_downloader")
        .map(|g| u64::from(g.steam_price_cents.unwrap_or(0)))
        .sum()
}

fn spawn_startup_turbo_price_resolution(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let candidates: Vec<(String, String, Option<u32>)> = {
            let Ok(lib) = load_library_from_disk(&app) else {
                return;
            };
            lib.into_iter()
                .filter(|g| {
                    g.acquisition_source == "turbo_downloader"
                        && g.steam_price_cents.unwrap_or(0) == 0
                })
                .map(|g| {
                    let aid = g
                        .app_id
                        .or_else(|| g.steam_app_id.and_then(|v| u32::try_from(v).ok()));
                    (g.id, g.title, aid)
                })
                .collect()
        };

        if candidates.is_empty() {
            return;
        }

        let mut updates: HashMap<String, (Option<u32>, u32, String)> = HashMap::new();
        for (gid, title, aid_opt) in candidates {
            let (resolved_aid, cents, fmt) =
                resolve_game_retail_price_fast(aid_opt, &title).await;
            if cents > 0 || (aid_opt.is_none() && resolved_aid.is_some()) {
                updates.insert(gid, (resolved_aid, cents, fmt));
            }
        }

        if updates.is_empty() {
            return;
        }

        let app_for_save = app.clone();
        if let Ok(Ok(updated_lib)) = tokio::task::spawn_blocking(move || -> Result<Vec<GameEntry>, String> {
            let mut library = load_library_from_disk(&app_for_save)?;
            let mut changed = false;
            for entry in &mut library {
                if let Some((aid_opt, cents, fmt)) = updates.get(&entry.id) {
                    if *cents > 0 && entry.steam_price_cents.unwrap_or(0) == 0 {
                        entry.steam_price_cents = Some(*cents);
                        entry.steam_price_formatted = Some(fmt.clone());
                        changed = true;
                    }
                    if let Some(aid) = aid_opt {
                        if entry.app_id.is_none() {
                            entry.app_id = Some(*aid);
                            entry.steam_app_id = Some(*aid as u64);
                            changed = true;
                        }
                    }
                }
            }
            if changed {
                save_library_to_disk(&app_for_save, &library)?;
            }
            Ok(library)
        })
        .await
        {
            let _ = app.emit("library-updated", &updated_lib);
        }
    });
}

#[tauri::command]
async fn add_local_game(
    app: AppHandle,
    exe_path: String,
    custom_title: Option<String>,
    custom_cover: Option<String>,
    genre: Option<String>,
    steam_app_id: Option<u32>,
) -> Result<GameEntry, String> {
    add_local_game_with_origin(
        app,
        exe_path,
        custom_title,
        custom_cover,
        genre,
        steam_app_id,
        Some("manual"),
    )
    .await
}

async fn add_local_game_with_origin(
    app: AppHandle,
    exe_path: String,
    custom_title: Option<String>,
    custom_cover: Option<String>,
    genre: Option<String>,
    steam_app_id: Option<u32>,
    origin: Option<&str>,
) -> Result<GameEntry, String> {
    let raw_trimmed = exe_path.trim().trim_matches('"').to_string();
    let raw_exe_file = PathBuf::from(&raw_trimmed);

    if !raw_exe_file.exists() || !raw_exe_file.is_file() {
        return Err(format!(
            "Executable file not found at '{}'. Please select a valid .exe binary.",
            raw_trimmed
        ));
    }

    let exe_file = canonicalize_drop_path(&raw_exe_file);
    let trimmed_path = exe_file.to_string_lossy().to_string();

    let install_dir = resolve_game_root_dir(&exe_file);
    let install_dir_str = install_dir.to_string_lossy().to_string();
    let inferred_name = install_dir
        .file_name()
        .and_then(|n| n.to_str())
        .or_else(|| exe_file.file_stem().and_then(|s| s.to_str()))
        .unwrap_or("Local Game");

    let mut title = custom_title
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| clean_game_title(inferred_name));

    let dir_for_size = install_dir.clone();
    let size_bytes = tokio::task::spawn_blocking(move || calculate_dir_size_sync(&dir_for_size))
        .await
        .map_err(|e| format!("Failed to calculate directory size: {}", e))?;

    let id = format!(
        "game_{}",
        trimmed_path
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
    );
    let size_formatted = format_bytes(size_bytes);
    let explicit_appid = steam_app_id.filter(|&id| id > 0);

    let (cover_url, detected_steam_app_id, official_name) =
        match custom_cover.filter(|c| !c.trim().is_empty()) {
            Some(manual_cover) => (manual_cover, explicit_appid.map(|id| id as u64), None),
            None => {
                if let Some(aid) = explicit_appid {
                    let (cover_opt, name_opt) = fetch_steam_app_cover_verified(aid as u64).await;
                    if let Some(valid_cover) = cover_opt {
                        (valid_cover, Some(aid as u64), name_opt)
                    } else {
                        let (fallback_cover, fallback_id, fallback_name) = smart_find_game_cover(
                            &app,
                            &id,
                            &title,
                            &trimmed_path,
                            &install_dir_str,
                        )
                        .await;
                        (
                            fallback_cover,
                            Some(aid as u64).or(fallback_id),
                            name_opt.or(fallback_name),
                        )
                    }
                } else {
                    smart_find_game_cover(&app, &id, &title, &trimmed_path, &install_dir_str).await
                }
            }
        };

    if let Some(real_title) = official_name {
        if title.len() <= 12 || !title.contains(' ') {
            title = real_title;
        }
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut app_id_u32 =
        explicit_appid.or_else(|| detected_steam_app_id.and_then(|sid| u32::try_from(sid).ok()));
    let (resolved_aid, resolved_cents, resolved_fmt) =
        resolve_game_retail_price_fast(app_id_u32, &title).await;
    if app_id_u32.is_none() {
        app_id_u32 = resolved_aid;
    }
    let final_steam_app_id = detected_steam_app_id.or_else(|| app_id_u32.map(|id| id as u64));
    let (steam_price_cents, steam_price_formatted) = (Some(resolved_cents), Some(resolved_fmt));

    let acq_origin = origin
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("manual")
        .to_string();
    let display_source = if acq_origin == "turbo_downloader" {
        "Turbo Downloader".to_string()
    } else {
        "Portable".to_string()
    };

    let new_entry = GameEntry {
        id: id.clone(),
        title,
        exe_path: trimmed_path.clone(),
        install_dir: install_dir_str,
        size_bytes,
        size_formatted,
        cover_url,
        hero_url: app_id_u32.map(|aid| {
            format!(
                "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{}/library_hero.jpg",
                aid
            )
        }),
        icon_path: None,
        genre: genre
            .filter(|g| !g.trim().is_empty())
            .unwrap_or_else(|| "Ready".to_string()),
        platform: "Windows".to_string(),
        source: display_source,
        acquisition_source: acq_origin,
        is_favorite: false,
        is_installed: true,
        added_at: now,
        last_played: None,
        steam_app_id: final_steam_app_id,
        app_id: app_id_u32,
        playtime_seconds: 0,
        imported_playtime_secs: 0,
        duke_playtime_secs: 0,
        last_played_ts: None,
        steam_price_cents,
        steam_price_formatted,
        launch_args: None,
        sandbox_enabled: false,
        nvme_prefetch_enabled: None,
        ..Default::default()
    };

    let app_clone = app.clone();
    let entry_clone = new_entry.clone();
    let exe_check_path = PathBuf::from(&trimmed_path);
    let unblock_dir = install_dir.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let q_dir = if cfg!(test) {
            None
        } else {
            pe_inspect::quarantine_root_dir_standalone().ok()
        };
        if let Ok(shield) = pe_inspect::inspect_pe_security_shield_file(&exe_check_path, false, None) {
            if shield.shield_badge == "Blocked" {
                let reasons: Vec<String> = shield
                    .suspicious_behaviors
                    .iter()
                    .map(|f| format!("{}: {}", f.title, f.explanation))
                    .collect();
                let q_note = q_dir
                    .as_deref()
                    .and_then(|qd| pe_inspect::quarantine_high_risk_binary(&exe_check_path, qd, &reasons, None).ok())
                    .map(|p| format!(" Quarantined to {}", p.display()))
                    .unwrap_or_default();
                return Err(format!(
                    "Security Shield v2 blocked '{}': {}.{}",
                    entry_clone.title,
                    reasons.join(" | "),
                    q_note
                ));
            }
        }
        let dir_shield = pe_inspect::gate_game_directory_pe_security(&unblock_dir, q_dir.as_deref(), 48);
        if let Some((b_path, b_reason, b_q)) = dir_shield.blocked_files.first() {
            let b_name = b_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("binary");
            let q_note = b_q
                .as_ref()
                .map(|p| format!(" (quarantined to {})", p.display()))
                .unwrap_or_default();
            return Err(format!(
                "Security Shield v2 blocked suspicious file '{}' in game directory{}: {}",
                b_name, q_note, b_reason
            ));
        }
        unblock_ntfs_zone_identifier(&unblock_dir);
        let mut library = load_library_from_disk(&app_clone)?;
        if let Some(existing) = library.iter_mut().find(|g| {
            g.id == entry_clone.id
                || g.exe_path.eq_ignore_ascii_case(&entry_clone.exe_path)
                || canonicalize_drop_path(Path::new(&g.exe_path))
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&entry_clone.exe_path)
        }) {
            let preserve_fav = existing.is_favorite;
            let preserve_last_played = existing.last_played_ts.or(existing.last_played);
            let preserve_imported_pt = existing.imported_playtime_secs;
            let preserve_duke_pt = if existing.duke_playtime_secs > 0 {
                existing.duke_playtime_secs
            } else if existing.imported_playtime_secs == 0 {
                existing.playtime_seconds
            } else {
                0
            };
            let preserve_playtime = preserve_imported_pt.saturating_add(preserve_duke_pt);
            let preserve_price_cents = match (entry_clone.steam_price_cents, existing.steam_price_cents) {
                (Some(new_c), Some(old_c)) if new_c == 0 && old_c > 0 => Some(old_c),
                (new_opt, old_opt) => new_opt.or(old_opt),
            };
            let preserve_price_fmt = preserve_price_cents
                .map(|c| format!("${:.2}", c as f64 / 100.0))
                .or_else(|| existing.steam_price_formatted.clone());
            let preserve_args = existing.launch_args.clone();
            let preserve_sandbox = existing.sandbox_enabled;
            let preserve_prefetch = existing.nvme_prefetch_enabled;
            let preserve_acq = if existing.acquisition_source == "turbo_downloader"
                || entry_clone.acquisition_source == "turbo_downloader"
            {
                "turbo_downloader".to_string()
            } else if !existing.acquisition_source.trim().is_empty() {
                existing.acquisition_source.clone()
            } else {
                entry_clone.acquisition_source.clone()
            };
            let preserve_src = if preserve_acq == "turbo_downloader" {
                "Turbo Downloader".to_string()
            } else {
                existing.source.clone()
            };
            let preserve_ver = existing.version_tag.clone();
            let preserve_feed = existing.update_feed_url.clone();
            let preserve_latest = existing.latest_version_tag.clone();
            let preserve_upd = existing.update_available;
            let preserve_ach = existing.achievements_path.clone();
            let old_id = existing.id.clone();
            *existing = entry_clone.clone();
            existing.is_favorite = preserve_fav;
            existing.last_played = preserve_last_played;
            existing.last_played_ts = preserve_last_played;
            existing.imported_playtime_secs = preserve_imported_pt;
            existing.duke_playtime_secs = preserve_duke_pt;
            existing.playtime_seconds = preserve_playtime;
            existing.steam_price_cents = preserve_price_cents;
            existing.steam_price_formatted = preserve_price_fmt;
            existing.launch_args = preserve_args;
            existing.sandbox_enabled = preserve_sandbox;
            existing.nvme_prefetch_enabled = preserve_prefetch;
            existing.acquisition_source = preserve_acq;
            if !preserve_src.trim().is_empty() {
                existing.source = preserve_src;
            }
            existing.version_tag = preserve_ver;
            existing.update_feed_url = preserve_feed;
            existing.latest_version_tag = preserve_latest;
            existing.update_available = preserve_upd;
            existing.achievements_path = preserve_ach;
            sidebar_icons::invalidate_sidebar_icon(&app_clone, &old_id);
        } else {
            library.push(entry_clone.clone());
        }
        save_library_to_disk(&app_clone, &library)?;
        sidebar_icons::invalidate_sidebar_icon(&app_clone, &entry_clone.id);
        Ok(())
    })
    .await
    .map_err(|e| format!("Failed to save library state: {}", e))??;

    trim_process_working_set();
    Ok(new_entry)
}

/// Parses a CLI argument string (e.g. `-dx11 -novid -mod "My Mod"`) into discrete tokens.
fn parse_cli_launch_args(raw: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in raw.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

/// Links or adds a local game executable with an explicit Steam App ID (alias of `add_local_game`).
#[tauri::command]
async fn link_local_game_with_steam_id(
    app: AppHandle,
    exe_path: String,
    custom_title: Option<String>,
    genre: Option<String>,
    steam_app_id: Option<u32>,
) -> Result<GameEntry, String> {
    add_local_game(app, exe_path, custom_title, None, genre, steam_app_id).await
}

/// Persists per-game custom CLI launch arguments (e.g. `-dx11 -novid -high`) into `library.json`.
#[tauri::command]
async fn set_game_launch_args(
    app: AppHandle,
    game_id: String,
    launch_args: Option<String>,
) -> Result<Vec<GameEntry>, String> {
    let cleaned_args = launch_args
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let app_clone = app.clone();
    let updated = tokio::task::spawn_blocking(move || -> Result<Vec<GameEntry>, String> {
        let mut library = load_library_from_disk(&app_clone)?;
        let found = library
            .iter_mut()
            .find(|g| g.id == game_id || g.exe_path.eq_ignore_ascii_case(&game_id));
        if let Some(entry) = found {
            entry.launch_args = cleaned_args;
        } else {
            return Err("Game entry not found in library.".to_string());
        }
        save_library_to_disk(&app_clone, &library)?;
        let _ = app_clone.emit("library-updated", &library);
        Ok(library)
    })
    .await
    .map_err(|e| format!("Failed to save launch arguments: {}", e))??;

    Ok(updated)
}

#[tauri::command]
async fn set_game_launch_profile(
    app: AppHandle,
    game_id: String,
    sandbox_enabled: Option<bool>,
    nvme_prefetch_override: Option<String>,
) -> Result<Vec<GameEntry>, String> {
    let app_clone = app.clone();
    let updated = tokio::task::spawn_blocking(move || -> Result<Vec<GameEntry>, String> {
        let mut library = load_library_from_disk(&app_clone)?;
        let found = library
            .iter_mut()
            .find(|g| g.id == game_id || g.exe_path.eq_ignore_ascii_case(&game_id));
        if let Some(entry) = found {
            if let Some(sb) = sandbox_enabled {
                entry.sandbox_enabled = sb;
                if sb {
                    let gdir = if !entry.install_dir.trim().is_empty() {
                        PathBuf::from(entry.install_dir.trim())
                    } else {
                        resolve_game_root_dir(Path::new(entry.exe_path.trim()))
                    };
                    if gdir.is_dir() {
                        let _ = pe_inspect::prepare_game_sandbox_env(&gdir);
                    }
                }
            }
            if let Some(mode) = nvme_prefetch_override {
                entry.nvme_prefetch_enabled = match mode.trim().to_ascii_lowercase().as_str() {
                    "enabled" | "on" | "true" => Some(true),
                    "disabled" | "off" | "false" => Some(false),
                    _ => None,
                };
            }
        } else {
            return Err("Game entry not found in library.".to_string());
        }
        save_library_to_disk(&app_clone, &library)?;
        let _ = app_clone.emit("library-updated", &library);
        Ok(library)
    })
    .await
    .map_err(|e| format!("Failed updating launch profile: {}", e))??;

    Ok(updated)
}

/// 1-Click NTFS LZX / XPRESS8K Game Folder Compression (`compact.exe /c /s:"<game_folder>" /a /i /exe:xpress8k`).
/// Compresses the game installation directory transparently on Windows 11 with zero runtime FPS penalty,
/// recalculates physical on-disk size via `GetCompressedFileSizeW`, and updates `size_bytes` in `library.json`.
pub(crate) async fn compress_game_folder_core(app: AppHandle, game_id: String) -> Result<Value, String> {
    compress_game_folder(app, game_id).await
}

#[tauri::command]
async fn compress_game_folder(app: AppHandle, game_id: String) -> Result<Value, String> {
    let current_library = load_library_from_disk(&app)?;
    let target_entry = current_library
        .iter()
        .find(|g| g.id == game_id || g.exe_path.eq_ignore_ascii_case(&game_id))
        .cloned()
        .ok_or_else(|| "Game entry not found in library.".to_string())?;

    let game_dir = if !target_entry.install_dir.trim().is_empty()
        && Path::new(&target_entry.install_dir).exists()
    {
        PathBuf::from(&target_entry.install_dir)
    } else {
        resolve_game_root_dir(Path::new(&target_entry.exe_path))
    };

    if !game_dir.exists() || !game_dir.is_dir() {
        return Err(format!(
            "Game installation directory does not exist: {}",
            game_dir.display()
        ));
    }

    let app_clone = app.clone();
    let gid_clone = target_entry.id.clone();
    let initial_recorded_size = target_entry.size_bytes;

    tokio::task::spawn_blocking(move || -> Result<Value, String> {
        let report = ntfs_compress::run_wof_operation_sync(
            Some(&app_clone),
            &gid_clone,
            &game_dir,
            true,
            ntfs_compress::WofAlgorithm::Xpress8k,
        );
        let after_bytes = report.compressed_bytes;
        let saved_bytes = report.saved_bytes;
        let after_formatted = format_bytes(after_bytes);
        let saved_formatted = format_bytes(saved_bytes);

        let mut library = load_library_from_disk(&app_clone)?;
        if let Some(entry) = library.iter_mut().find(|g| g.id == gid_clone) {
            entry.size_bytes = after_bytes;
            entry.size_formatted = after_formatted.clone();
        }
        save_library_to_disk(&app_clone, &library)?;
        let _ = app_clone.emit("library-updated", &library);
        trim_process_working_set();

        Ok(serde_json::json!({
            "gameId": gid_clone,
            "beforeBytes": initial_recorded_size.max(report.logical_bytes),
            "afterBytes": after_bytes,
            "savedBytes": saved_bytes,
            "savedFormatted": saved_formatted,
            "newSizeFormatted": after_formatted,
            "library": library
        }))
    })
    .await
    .map_err(|e| format!("NTFS compression task failed: {}", e))?
}

/// Opens a native file picker (or accepts `new_exe_path`) so users can switch which `.exe` a library entry launches
/// (e.g., DX11 vs DX12, Vulkan, or Mod Launcher).
#[tauri::command]
async fn change_game_exe(
    app: AppHandle,
    game_id: String,
    new_exe_path: Option<String>,
) -> Result<Vec<GameEntry>, String> {
    let current_library = load_library_from_disk(&app)?;
    let target_entry = current_library
        .iter()
        .find(|g| g.id == game_id || g.exe_path.eq_ignore_ascii_case(&game_id))
        .cloned()
        .ok_or_else(|| "Game entry not found in library.".to_string())?;

    let chosen_path_str = match new_exe_path.filter(|p| !p.trim().is_empty()) {
        Some(p) => p,
        None => {
            let start_dir = PathBuf::from(&target_entry.install_dir);
            let app_for_dialog = app.clone();
            let picked = tokio::task::spawn_blocking(move || {
                let mut builder = app_for_dialog
                    .dialog()
                    .file()
                    .set_title("Select Game Executable (.exe)")
                    .add_filter("Game Executable", &["exe", "bat", "cmd"]);
                if start_dir.exists() {
                    builder = builder.set_directory(start_dir);
                }
                builder.blocking_pick_file()
            })
            .await
            .map_err(|e| format!("File picker failed: {}", e))?;

            match picked {
                Some(file_path) => file_path.to_string(),
                None => return Ok(current_library),
            }
        }
    };

    let exe_file = PathBuf::from(chosen_path_str.trim().trim_matches('"'));
    if !exe_file.exists() || !exe_file.is_file() {
        return Err(format!(
            "Selected executable does not exist: {}",
            exe_file.display()
        ));
    }

    let new_install_dir = resolve_game_root_dir(&exe_file);
    let dir_for_size = new_install_dir.clone();
    let size_bytes = tokio::task::spawn_blocking(move || calculate_dir_size_sync(&dir_for_size))
        .await
        .map_err(|e| format!("Failed to calculate directory size: {}", e))?;

    let mut library = load_library_from_disk(&app)?;
    let mut updated_id: Option<String> = None;
    if let Some(entry) = library
        .iter_mut()
        .find(|g| g.id == game_id || g.exe_path.eq_ignore_ascii_case(&game_id))
    {
        entry.exe_path = exe_file.to_string_lossy().to_string();
        entry.install_dir = new_install_dir.to_string_lossy().to_string();
        entry.size_bytes = size_bytes;
        entry.size_formatted = format_bytes(size_bytes);
        entry.is_installed = true;
        updated_id = Some(entry.id.clone());
    }

    save_library_to_disk(&app, &library)?;
    if let Some(gid) = updated_id {
        sidebar_icons::invalidate_sidebar_icon(&app, &gid);
    }
    let _ = app.emit("library-updated", &library);
    Ok(library)
}

async fn launch_game_internal(
    app: AppHandle,
    window: Option<WebviewWindow>,
    exe_path: String,
    game_id: Option<String>,
    explicit_launch_args: Option<String>,
) -> Result<(), String> {
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
    if let Ok(lib) = load_library_standalone() {
        if let Some(matched) = lib.iter().find(|g| {
            game_id.as_ref().map(|id| g.id == *id).unwrap_or(false)
                || g.exe_path.eq_ignore_ascii_case(&trimmed)
        }) {
            achievements::start_achievement_watcher(app.clone(), matched);
            time_machine::start_save_watcher(app.clone(), matched);
        }
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
        if let Some(ref gid) = gid_after {
            achievements::stop_achievement_watcher(gid);
            time_machine::stop_save_watcher(gid);
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
                    let _ = save_library_to_disk(&app_disk, &library);
                    let _ = app_disk.emit("library-updated", &library);
                }
            }
        })
        .await;

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

fn spawn_hotkey_watcher_if_needed(app: AppHandle) {
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
            if AUTO_FREEZE_MINIMIZED.load(Ordering::Relaxed) && ticks_elapsed > 125 && ticks_elapsed % 10 == 0 {
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
async fn suspend_or_resume_game(
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
async fn get_active_game_sessions() -> Vec<ActiveGameSession> {
    get_active_sessions()
        .lock()
        .map(|g| g.values().cloned().collect())
        .unwrap_or_default()
}

#[tauri::command]
fn poll_gamepad_state() -> duke_core::win32_ffi::XInputPadState {
    duke_core::win32_ffi::poll_xinput_state()
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
async fn launch_local_game(
    app: AppHandle,
    window: WebviewWindow,
    exe_path: String,
    game_id: Option<String>,
    launch_args: Option<String>,
) -> Result<(), String> {
    launch_game_internal(app, Some(window), exe_path, game_id, launch_args).await
}

#[tauri::command]
async fn launch_game(
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

fn build_steam_hero_candidates(appid: u32, _header_image: Option<&str>) -> Vec<String> {
    if appid == 0 {
        return Vec::new();
    }
    vec![
        format!(
            "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{}/library_hero.jpg",
            appid
        ),
        format!(
            "https://cdn.cloudflare.steamstatic.com/steam/apps/{}/library_hero.jpg",
            appid
        ),
        format!(
            "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{}/header.jpg",
            appid
        ),
    ]
}

fn decode_basic_html_entities(input: &str) -> String {
    input
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
}

fn strip_steam_clan_tokens(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut idx = 0usize;
    let bytes = input.as_bytes();

    while idx < input.len() {
        let rest = &input[idx..];
        let is_clan_token = rest.len() >= 7
            && rest.as_bytes()[0] == b'{'
            && rest
                .get(..7)
                .map(|p| p.eq_ignore_ascii_case("{STEAM_"))
                .unwrap_or(false);

        if is_clan_token {
            if let Some(close_rel) = rest.find('}') {
                idx += close_rel + 1;
                if idx < bytes.len() && (bytes[idx] == b'/' || bytes[idx] == b'\\') {
                    while idx < bytes.len() {
                        let b = bytes[idx];
                        if b.is_ascii_whitespace()
                            || matches!(b, b'[' | b']' | b'<' | b'>' | b'"' | b'\'' | b'(' | b')')
                        {
                            break;
                        }
                        idx += 1;
                    }
                }
                while idx < input.len() && !input.is_char_boundary(idx) {
                    idx += 1;
                }
                if !out.ends_with(' ') && !out.ends_with('\n') && !out.is_empty() {
                    out.push(' ');
                }
                continue;
            } else {
                // Truncated `{STEAM_...` token at the end of a clipped summary
                break;
            }
        }

        if let Some(ch) = rest.chars().next() {
            out.push(ch);
            idx += ch.len_utf8();
        } else {
            break;
        }
    }

    out
}

fn strip_bbcode_media_blocks(input: &str) -> String {
    let mut current = input.to_string();
    for tag in ["img", "previewyoutube", "video", "youtube"] {
        let open_prefix = format!("[{}", tag);
        let close_tag = format!("[/{}]", tag);
        let mut next = String::with_capacity(current.len());
        let mut idx = 0usize;

        while idx < current.len() {
            let rest = &current[idx..];
            let matches_open = rest.len() > open_prefix.len()
                && rest
                    .get(..open_prefix.len())
                    .map(|p| p.eq_ignore_ascii_case(&open_prefix))
                    .unwrap_or(false)
                && matches!(rest.as_bytes()[open_prefix.len()], b']' | b'=' | b' ');

            if matches_open {
                let lower_rest = rest.to_ascii_lowercase();
                if let Some(end_rel) = lower_rest.find(&close_tag) {
                    idx += end_rel + close_tag.len();
                    if !next.ends_with(' ') && !next.ends_with('\n') && !next.is_empty() {
                        next.push(' ');
                    }
                    continue;
                }
                if let Some(open_end) = rest.find(']') {
                    idx += open_end + 1;
                    continue;
                }
            }

            if let Some(ch) = rest.chars().next() {
                next.push(ch);
                idx += ch.len_utf8();
            } else {
                break;
            }
        }
        current = next;
    }
    current
}

fn convert_html_to_bbcode_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut idx = 0usize;

    while idx < input.len() {
        let rest = &input[idx..];
        if rest.starts_with('<') {
            if let Some(close_rel) = rest.find('>') {
                let tag_inner = rest[1..close_rel].trim();
                let lower = tag_inner.to_ascii_lowercase();
                let tag_name = lower
                    .trim_start_matches('/')
                    .split(|c: char| c.is_whitespace() || c == '/')
                    .next()
                    .unwrap_or("");
                let is_closing = lower.starts_with('/');

                match (is_closing, tag_name) {
                    (false, "br") => out.push('\n'),
                    (false, "p") | (false, "div") => {}
                    (true, "p") | (true, "div") => out.push_str("\n\n"),
                    (false, "b") | (false, "strong") => out.push_str("[b]"),
                    (true, "b") | (true, "strong") => out.push_str("[/b]"),
                    (false, "i") | (false, "em") => out.push_str("[i]"),
                    (true, "i") | (true, "em") => out.push_str("[/i]"),
                    (false, "h1") => out.push_str("[h1]"),
                    (true, "h1") => out.push_str("[/h1]\n"),
                    (false, "h2") => out.push_str("[h2]"),
                    (true, "h2") => out.push_str("[/h2]\n"),
                    (false, "h3") => out.push_str("[h3]"),
                    (true, "h3") => out.push_str("[/h3]\n"),
                    (false, "ul") | (false, "ol") => out.push_str("\n[list]"),
                    (true, "ul") | (true, "ol") => out.push_str("[/list]\n"),
                    (false, "li") => out.push_str("[*]"),
                    (true, "li") => out.push('\n'),
                    (false, "a") => {
                        let href = if let Some(pos) = lower.find("href=") {
                            let val_part = tag_inner[pos + 5..].trim_start();
                            if let Some(stripped) = val_part.strip_prefix('"') {
                                stripped.split('"').next().unwrap_or("")
                            } else if let Some(stripped) = val_part.strip_prefix('\'') {
                                stripped.split('\'').next().unwrap_or("")
                            } else {
                                val_part.split_whitespace().next().unwrap_or("")
                            }
                        } else {
                            ""
                        };
                        if href.starts_with("https://") || href.starts_with("http://") {
                            out.push_str(&format!("[url={}]", href));
                        }
                    }
                    (true, "a") => out.push_str("[/url]"),
                    _ => out.push(' '),
                }

                idx += close_rel + 1;
                continue;
            }
        }

        if let Some(ch) = rest.chars().next() {
            out.push(ch);
            idx += ch.len_utf8();
        } else {
            break;
        }
    }

    out
}

fn clean_steam_clan_tokens(raw: &str) -> String {
    let no_tokens = strip_steam_clan_tokens(raw);
    let no_media = strip_bbcode_media_blocks(&no_tokens);
    let bbcode_text = convert_html_to_bbcode_text(&no_media);
    let decoded = decode_basic_html_entities(&bbcode_text);

    // Collapse horizontal whitespace cleanly while preserving line breaks for headings/lists
    let mut normalized = String::with_capacity(decoded.len());
    let mut consecutive_newlines = 0usize;
    for raw_line in decoded.lines() {
        let collapsed_line = raw_line.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed_line.is_empty() {
            if !normalized.is_empty() && consecutive_newlines < 2 {
                normalized.push('\n');
                consecutive_newlines += 1;
            }
        } else {
            if !normalized.is_empty() && consecutive_newlines == 0 {
                normalized.push('\n');
            }
            normalized.push_str(&collapsed_line);
            consecutive_newlines = 0;
        }
    }

    // Remove a dangling incomplete BBCode opening tag if clipped mid-tag
    let trimmed = normalized.trim();
    if let Some(last_open) = trimmed.rfind('[') {
        if trimmed[last_open..].find(']').is_none() {
            return trimmed[..last_open].trim().to_string();
        }
    }
    trimmed.to_string()
}

fn strip_bbcode_and_html(raw: &str) -> String {
    let token_clean = strip_steam_clan_tokens(raw);
    let mut out = String::with_capacity(token_clean.len());
    let mut in_angle = false;
    let mut in_bracket = false;

    for ch in token_clean.chars() {
        match ch {
            '<' => in_angle = true,
            '>' if in_angle => {
                in_angle = false;
                out.push(' ');
            }
            '[' => in_bracket = true,
            ']' if in_bracket => {
                in_bracket = false;
                out.push(' ');
            }
            _ if !in_angle && !in_bracket => out.push(ch),
            _ => {}
        }
    }

    let decoded = decode_basic_html_entities(&out);
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[tauri::command]
async fn get_game_steam_details(
    app: AppHandle,
    app_id: Option<u32>,
    title: String,
    install_dir: Option<String>,
    game_id: Option<String>,
) -> Result<GameSteamDetails, String> {
    let mut resolved_appid = app_id.filter(|&id| id > 0);

    if resolved_appid.is_none() {
        if let Some(ref dir_str) = install_dir {
            let dir_path = Path::new(dir_str);
            if let Some(folder_id) = detect_steam_appid_in_folder(dir_path) {
                resolved_appid = u32::try_from(folder_id).ok();
            }
        }
    }

    let mut matched_store_title: Option<String> = None;
    if resolved_appid.is_none() {
        if let Some((found_id, store_name)) = resolve_steam_appid_by_title(&title).await {
            resolved_appid = Some(found_id);
            matched_store_title = store_name;
        }
    }

    // Persist discovered app_id back to library.json so future opens are instant
    if let (Some(valid_id), Some(ref gid)) = (resolved_appid, &game_id) {
        let app_clone = app.clone();
        let gid_clone = gid.clone();
        let _ = tokio::task::spawn_blocking(move || {
            if let Ok(mut lib) = load_library_from_disk(&app_clone) {
                if let Some(entry) = lib.iter_mut().find(|g| g.id == gid_clone) {
                    if entry.app_id != Some(valid_id) {
                        entry.app_id = Some(valid_id);
                        entry.steam_app_id = Some(valid_id as u64);
                        let _ = save_library_to_disk(&app_clone, &lib);
                    }
                }
            }
        })
        .await;
    }

    let Some(appid) = resolved_appid else {
        if let Some(ref gid) = game_id {
            let app_clone = app.clone();
            let gid_clone = gid.clone();
            let _ = tokio::task::spawn_blocking(move || {
                if let Ok(mut lib) = load_library_from_disk(&app_clone) {
                    if let Some(entry) = lib.iter_mut().find(|g| g.id == gid_clone) {
                        if entry.steam_price_cents.is_none() {
                            entry.steam_price_cents = Some(0);
                            entry.steam_price_formatted = Some("$0.00".to_string());
                            let _ = save_library_to_disk(&app_clone, &lib);
                        }
                    }
                }
            })
            .await;
        }
        return Ok(GameSteamDetails {
            app_id: None,
            title: clean_game_title(&title),
            short_description: "Standalone PC title installed locally and managed by Duke Launcher. Launch directly with zero launcher overhead.".to_string(),
            developers: vec!["Local Studio / Standalone".to_string()],
            publishers: vec!["DRM-Free / Direct".to_string()],
            release_date: "Installed Locally".to_string(),
            genres: vec!["PC Game".to_string(), "Standalone".to_string()],
            price_final: "N/A (Non-Steam)".to_string(),
            price_initial: None,
            discount_percent: 0,
            is_free: false,
            steam_price_cents: Some(0),
            steam_price_formatted: Some("$0.00".to_string()),
            live_players: None,
            header_image: None,
            library_hero: None,
            hero_candidates: Vec::new(),
            news: Vec::new(),
        });
    };

    let details_url = format!(
        "https://store.steampowered.com/api/appdetails?appids={}&cc=us&l=en",
        appid
    );
    let players_url = format!(
        "https://api.steampowered.com/ISteamUserStats/GetNumberOfCurrentPlayers/v1/?appid={}",
        appid
    );
    let news_url = format!(
        "https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/?appid={}&count=6&maxlength=1200&format=json",
        appid
    );

    let details_fut = async { fetch_doh_json(&details_url).await.ok() };
    let players_fut = async { fetch_doh_json(&players_url).await.ok() };
    let news_fut = async { fetch_doh_json(&news_url).await.ok() };

    let (details_json, players_json, news_json) = tokio::join!(details_fut, players_fut, news_fut);

    let mut final_title = matched_store_title.unwrap_or_else(|| clean_game_title(&title));
    let mut short_description =
        "Official Steam title indexed in your local Duke Launcher library.".to_string();
    let mut developers = Vec::new();
    let mut publishers = Vec::new();
    let mut release_date = "Available Now".to_string();
    let mut genres = Vec::new();
    let mut price_final = "View on Steam".to_string();
    let mut price_initial: Option<String> = None;
    let mut discount_percent: u32 = 0;
    let mut is_free = false;
    let mut retail_price_cents: u32 = 0;
    let mut retail_price_formatted = "$0.00".to_string();
    let mut header_image: Option<String> = None;
    let library_hero = Some(format!(
        "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{}/library_hero.jpg",
        appid
    ));

    if let Some(ref d_root) = details_json {
        if let Some(data) = d_root.get(appid.to_string()).and_then(|v| v.get("data")) {
            let (cents, fmt_str) = extract_steam_price_from_appdetails_data(data);
            retail_price_cents = cents;
            retail_price_formatted = fmt_str;

            if let Some(name) = data.get("name").and_then(|v| v.as_str()) {
                if !name.trim().is_empty() {
                    final_title = name.to_string();
                }
            }
            if let Some(desc) = data.get("short_description").and_then(|v| v.as_str()) {
                let cleaned = strip_bbcode_and_html(desc);
                if !cleaned.is_empty() {
                    short_description = cleaned;
                }
            }
            if let Some(devs) = data.get("developers").and_then(|v| v.as_array()) {
                developers = devs
                    .iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect();
            }
            if let Some(pubs) = data.get("publishers").and_then(|v| v.as_array()) {
                publishers = pubs
                    .iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect();
            }
            if let Some(rel) = data
                .get("release_date")
                .and_then(|r| r.get("date"))
                .and_then(|d| d.as_str())
            {
                if !rel.trim().is_empty() {
                    release_date = rel.to_string();
                }
            }
            if let Some(g_arr) = data.get("genres").and_then(|v| v.as_array()) {
                genres = g_arr
                    .iter()
                    .filter_map(|g| {
                        g.get("description")
                            .and_then(|d| d.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect();
            }
            if let Some(h_img) = data.get("header_image").and_then(|v| v.as_str()) {
                header_image = Some(h_img.to_string());
            }

            is_free = data
                .get("is_free")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if is_free {
                price_final = "Free to Play".to_string();
            } else if let Some(po) = data.get("price_overview") {
                if let Some(fin) = po.get("final_formatted").and_then(|v| v.as_str()) {
                    price_final = fin.to_string();
                }
                if let Some(init) = po.get("initial_formatted").and_then(|v| v.as_str()) {
                    if !init.is_empty() && init != price_final {
                        price_initial = Some(init.to_string());
                    }
                }
                discount_percent = po
                    .get("discount_percent")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
            }
        }
    }

    if let Some(ref gid) = game_id {
        let app_clone = app.clone();
        let gid_clone = gid.clone();
        let fmt_clone = retail_price_formatted.clone();
        let _ = tokio::task::spawn_blocking(move || {
            if let Ok(mut lib) = load_library_from_disk(&app_clone) {
                if let Some(entry) = lib.iter_mut().find(|g| g.id == gid_clone) {
                    if entry.app_id != Some(appid)
                        || entry.steam_price_cents != Some(retail_price_cents)
                    {
                        entry.app_id = Some(appid);
                        entry.steam_app_id = Some(appid as u64);
                        entry.steam_price_cents = Some(retail_price_cents);
                        entry.steam_price_formatted = Some(fmt_clone);
                        let _ = save_library_to_disk(&app_clone, &lib);
                        let _ = app_clone.emit("library-updated", &lib);
                    }
                }
            }
        })
        .await;
    }

    let live_players = players_json
        .as_ref()
        .and_then(|p| p.get("response"))
        .and_then(|r| r.get("player_count"))
        .and_then(|c| c.as_u64());

    let mut news = Vec::new();
    if let Some(ref n_root) = news_json {
        if let Some(items) = n_root
            .get("appnews")
            .and_then(|a| a.get("newsitems"))
            .and_then(|i| i.as_array())
        {
            for item in items.iter().take(6) {
                let item_title = item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Steam Update")
                    .to_string();
                let date = item.get("date").and_then(|v| v.as_u64()).unwrap_or(0);
                let raw_contents = item.get("contents").and_then(|v| v.as_str()).unwrap_or("");
                let cleaned_contents = clean_steam_clan_tokens(raw_contents);
                let feedlabel = item
                    .get("feedlabel")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or("Announcement")
                    .to_string();
                let url = item
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                news.push(SteamNewsItem {
                    title: item_title,
                    date,
                    contents: cleaned_contents,
                    feedlabel,
                    url,
                });
            }
        }
    }

    let hero_candidates = build_steam_hero_candidates(appid, header_image.as_deref());

    Ok(GameSteamDetails {
        app_id: Some(appid),
        title: final_title,
        short_description,
        developers,
        publishers,
        release_date,
        genres,
        price_final,
        price_initial,
        discount_percent,
        is_free,
        steam_price_cents: Some(retail_price_cents),
        steam_price_formatted: Some(retail_price_formatted),
        live_players,
        header_image,
        library_hero,
        hero_candidates,
        news,
    })
}

#[tauri::command]
async fn sync_library_steam_prices(app: AppHandle) -> Result<Vec<GameEntry>, String> {
    if PRICE_SYNC_ACTIVE
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return tokio::task::spawn_blocking(move || load_library_from_disk(&app))
            .await
            .map_err(|e| format!("Failed to load local library: {}", e))?;
    }

    let app_for_load = app.clone();
    let pending: Vec<(String, String, String, Option<u32>)> = tokio::task::spawn_blocking(move || {
        let lib = load_library_from_disk(&app_for_load)?;
        Ok::<_, String>(
            lib.into_iter()
                .filter(|g| g.steam_price_cents.is_none())
                .map(|g| {
                    let aid = g
                        .app_id
                        .or_else(|| g.steam_app_id.and_then(|sid| u32::try_from(sid).ok()));
                    (g.id, g.title, g.install_dir, aid)
                })
                .collect(),
        )
    })
    .await
    .map_err(|e| {
        PRICE_SYNC_ACTIVE.store(false, Ordering::SeqCst);
        format!("Price sync task error: {}", e)
    })??;

    if pending.is_empty() {
        PRICE_SYNC_ACTIVE.store(false, Ordering::SeqCst);
        return tokio::task::spawn_blocking(move || load_library_from_disk(&app))
            .await
            .map_err(|e| format!("Failed to load local library: {}", e))?;
    }

    let mut resolved_updates: HashMap<String, (Option<u32>, u32, String)> = HashMap::new();

    for (gid, title, install_dir, existing_appid) in pending {
        let mut resolved_id = existing_appid.filter(|&id| id > 0);
        if resolved_id.is_none() {
            let dir_path = Path::new(&install_dir);
            if let Some(folder_id) = detect_steam_appid_in_folder(dir_path) {
                resolved_id = u32::try_from(folder_id).ok();
            }
        }
        if resolved_id.is_none() {
            if let Some((found_id, _)) = resolve_steam_appid_by_title(&title).await {
                resolved_id = Some(found_id);
            }
        }

        let (cents, formatted) = match resolved_id {
            Some(aid) => fetch_steam_app_retail_price(aid).await,
            None => (0, "$0.00".to_string()),
        };

        resolved_updates.insert(gid, (resolved_id, cents, formatted));
    }

    let app_for_save = app.clone();
    let updated_lib = tokio::task::spawn_blocking(move || -> Result<Vec<GameEntry>, String> {
        let mut lib = load_library_from_disk(&app_for_save)?;
        let mut changed = false;
        for entry in &mut lib {
            if let Some((aid_opt, cents, fmt_str)) = resolved_updates.get(&entry.id) {
                if let Some(aid) = *aid_opt {
                    entry.app_id = Some(aid);
                    entry.steam_app_id = Some(aid as u64);
                }
                entry.steam_price_cents = Some(*cents);
                entry.steam_price_formatted = Some(fmt_str.clone());
                changed = true;
            }
        }
        if changed {
            save_library_to_disk(&app_for_save, &lib)?;
            let _ = app_for_save.emit("library-updated", &lib);
        }
        Ok(lib)
    })
    .await
    .map_err(|e| format!("Failed to persist Steam prices: {}", e));

    PRICE_SYNC_ACTIVE.store(false, Ordering::SeqCst);
    updated_lib?
}

fn json_val_to_u64(v: Option<&Value>) -> u64 {
    match v {
        Some(Value::Number(n)) => n
            .as_u64()
            .or_else(|| n.as_i64().map(|x| x.max(0) as u64))
            .or_else(|| n.as_f64().map(|x| x.max(0.0) as u64))
            .unwrap_or(0),
        Some(Value::String(s)) => {
            let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
            digits.parse::<u64>().unwrap_or(0)
        }
        _ => 0,
    }
}

fn json_val_to_u32(v: Option<&Value>) -> u32 {
    u32::try_from(json_val_to_u64(v).min(u32::MAX as u64)).unwrap_or(u32::MAX)
}

fn parse_steamspy_owners_band(raw: &str) -> (u64, u64) {
    let mut nums = Vec::new();
    for part in raw.split("..") {
        let digits: String = part.chars().filter(|c| c.is_ascii_digit()).collect();
        if let Ok(val) = digits.parse::<u64>() {
            nums.push(val);
        }
    }
    match (nums.first().copied(), nums.get(1).copied()) {
        (Some(min), Some(max)) => (min, max.max(min)),
        (Some(min), None) => (min, min),
        _ => (0, 0),
    }
}

fn format_compact_owners_band(min: u64, max: u64, raw_label: &str) -> String {
    if min == 0 && max == 0 {
        let trimmed = raw_label.trim();
        return if trimmed.is_empty() {
            "Unreported".to_string()
        } else {
            trimmed.to_string()
        };
    }
    let fmt = |v: u64| -> String {
        if v >= 1_000_000 {
            format!("{:.1}M", v as f64 / 1_000_000.0)
        } else if v >= 1_000 {
            format!("{}K", v / 1_000)
        } else {
            v.to_string()
        }
    };
    format!("{} - {}", fmt(min), fmt(max))
}

fn push_unique_label(list: &mut Vec<String>, candidate: &str) {
    let clean = candidate.trim();
    if clean.is_empty() {
        return;
    }
    if !list.iter().any(|existing| existing.eq_ignore_ascii_case(clean)) {
        list.push(clean.to_string());
    }
}

fn apply_tag_and_category_flags(entry: &mut CatalogGameEntry) {
    let all_labels: Vec<String> = entry
        .categories
        .iter()
        .chain(entry.tags.iter())
        .chain(entry.genres.iter())
        .map(|s| s.to_lowercase())
        .collect();

    let mut has_mmo_or_pvp_marker = false;
    for lbl in &all_labels {
        if lbl.contains("single-player") || lbl.contains("singleplayer") {
            entry.is_singleplayer = true;
        }
        if lbl.contains("co-op") || lbl == "coop" || lbl.contains("lan co-op") || lbl.contains("split screen") {
            entry.is_coop = true;
        }
        if lbl.contains("multi-player") || lbl.contains("multiplayer") || lbl.contains("pvp") || lbl.contains("mmo") {
            entry.is_multiplayer = true;
        }
        if lbl.contains("massively multiplayer")
            || lbl == "mmo"
            || lbl == "mmorpg"
            || lbl.contains("online pvp")
            || lbl.contains("battle royale")
            || lbl == "moba"
        {
            has_mmo_or_pvp_marker = true;
        }
    }

    entry.is_online_only_pvp =
        has_mmo_or_pvp_marker && !entry.is_singleplayer && !entry.is_coop;
}

fn compute_catalog_value_score(entry: &CatalogGameEntry) -> f64 {
    let total_reviews = entry.positive.saturating_add(entry.negative);
    let bayesian_ratio = if total_reviews > 0 {
        (entry.positive as f64 + 7.0) / (total_reviews as f64 + 10.0)
    } else {
        0.65
    };
    let confidence = ((total_reviews as f64 + 1.0).log10() / 4.5).min(1.0);
    let rating_pts = bayesian_ratio * (24.0 + 11.0 * confidence);

    let retail_cents = entry.initial_price_cents.max(entry.price_cents);
    let retail_dollars = (retail_cents as f64) / 100.0;
    let price_pts = (retail_dollars.min(70.0) / 70.0) * 28.0;

    let mode_pts = if entry.is_singleplayer && entry.is_coop {
        22.0
    } else if entry.is_singleplayer || entry.is_coop {
        16.0
    } else {
        0.0
    };

    let ccu_pts = (((entry.ccu as f64) + 1.0).log10().min(5.5) / 5.5) * 18.0;
    let owners_pts = (((entry.owners_min as f64) + 1.0).log10().min(7.5) / 7.5) * 7.0;

    let mut total = rating_pts + price_pts + mode_pts + ccu_pts + owners_pts;
    if entry.is_online_only_pvp {
        total -= 35.0;
    }
    if entry.price_cents == 0 || entry.is_free {
        total *= 0.25;
    }
    (total * 100.0).round() / 100.0
}

fn finalize_catalog_entry(entry: &mut CatalogGameEntry, now: u64) {
    if entry.app_id == 0 {
        return;
    }
    if entry.platform_id > 0 || platforms::is_synthetic_retro_appid(entry.app_id) {
        let plat = platforms::platform_by_id(entry.platform_id);
        entry.console_group = plat.console_group;
        if entry.platform_name.is_empty() {
            entry.platform_name = plat.display_name.to_string();
        }
        if entry.platform_slug.is_empty() {
            entry.platform_slug = plat.slug.to_string();
        }
        if entry.cover_url.is_empty() {
            let (boxart_url, _, _) =
                retro_catalog::build_libretro_thumbnail_urls(entry.platform_id, &entry.name);
            if !boxart_url.is_empty() {
                entry.cover_url = boxart_url;
            }
        }
        if entry.header_url.is_empty() {
            entry.header_url = entry.cover_url.clone();
        }
        if entry.hero_url.is_empty() {
            entry.hero_url = entry.cover_url.clone();
        }
        entry.is_free = true;
        if entry.price_formatted.is_empty() {
            entry.price_formatted = plat.short_name.to_string();
        }
        if entry.updated_at == 0 {
            entry.updated_at = now;
        }
        return;
    }
    entry.console_group = platforms::GROUP_PC;
    if entry.platform_name.is_empty() {
        entry.platform_name = "PC".to_string();
    }
    if entry.platform_slug.is_empty() {
        entry.platform_slug = "pc".to_string();
    }
    if entry.cover_url.is_empty() {
        entry.cover_url = format!(
            "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{}/library_600x900.jpg",
            entry.app_id
        );
    }
    if entry.header_url.is_empty() {
        entry.header_url = format!(
            "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{}/header.jpg",
            entry.app_id
        );
    }
    if entry.hero_url.is_empty() {
        entry.hero_url = format!(
            "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{}/library_hero.jpg",
            entry.app_id
        );
    }
    entry.is_free = entry.price_cents == 0 && entry.initial_price_cents == 0;
    if entry.price_formatted.is_empty() || (entry.price_formatted == "$0.00" && !entry.is_free) {
        entry.price_formatted = if entry.price_cents == 0 {
            "Free".to_string()
        } else {
            format!("${:.2}", entry.price_cents as f64 / 100.0)
        };
    }
    if entry.owners_min > 0 || entry.owners_max > 0 {
        entry.owners_label = format_compact_owners_band(entry.owners_min, entry.owners_max, &entry.owners_label);
    }
    let total_reviews = entry.positive.saturating_add(entry.negative);
    entry.review_pct = if total_reviews > 0 {
        ((entry.positive as f64 / total_reviews as f64) * 1000.0).round() / 10.0
    } else {
        0.0
    };
    apply_tag_and_category_flags(entry);
    entry.value_score = compute_catalog_value_score(entry);
    if entry.updated_at == 0 {
        entry.updated_at = now;
    }
}

fn merge_steamspy_map(
    entries_by_id: &mut HashMap<u32, CatalogGameEntry>,
    raw_map: &Value,
    forced_tag: Option<&str>,
    mark_sp: bool,
    mark_coop: bool,
    mark_genre: Option<&str>,
    now: u64,
) {
    let Some(obj) = raw_map.as_object() else {
        return;
    };

    for (key, item) in obj {
        let app_id = json_val_to_u32(item.get("appid"))
            .max(key.parse::<u32>().unwrap_or(0));
        if app_id == 0 {
            continue;
        }
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }

        let price_cents = json_val_to_u32(item.get("price"));
        let initial_price_cents = json_val_to_u32(item.get("initialprice")).max(price_cents);
        let discount_percent = json_val_to_u32(item.get("discount"));
        let ccu = json_val_to_u64(item.get("ccu"));
        let positive = json_val_to_u64(item.get("positive"));
        let negative = json_val_to_u64(item.get("negative"));
        let owners_str = item
            .get("owners")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let (owners_min, owners_max) = parse_steamspy_owners_band(owners_str);

        let entry = entries_by_id.entry(app_id).or_insert_with(|| CatalogGameEntry {
            app_id,
            name: name.clone(),
            ..Default::default()
        });

        if !name.is_empty() && entry.name.is_empty() {
            entry.name = name;
        }
        if price_cents > 0 || !entry.details_enriched {
            entry.price_cents = price_cents;
            entry.initial_price_cents = initial_price_cents;
            entry.discount_percent = discount_percent;
            entry.price_formatted = if price_cents == 0 {
                "Free".to_string()
            } else {
                format!("${:.2}", price_cents as f64 / 100.0)
            };
        }
        if ccu > 0 || entry.ccu == 0 {
            entry.ccu = ccu;
        }
        if owners_max > entry.owners_max {
            entry.owners_min = owners_min;
            entry.owners_max = owners_max;
            entry.owners_label = format_compact_owners_band(owners_min, owners_max, owners_str);
        }
        if positive.saturating_add(negative) >= entry.positive.saturating_add(entry.negative) {
            entry.positive = positive;
            entry.negative = negative;
        }

        if let Some(dev) = item.get("developer").and_then(|v| v.as_str()) {
            for d in dev.split(',') {
                push_unique_label(&mut entry.developers, d);
            }
        }
        if let Some(pub_str) = item.get("publisher").and_then(|v| v.as_str()) {
            for p in pub_str.split(',') {
                push_unique_label(&mut entry.publishers, p);
            }
        }
        if let Some(genre_str) = item.get("genre").and_then(|v| v.as_str()) {
            for g in genre_str.split(',') {
                push_unique_label(&mut entry.genres, g);
            }
        }
        if let Some(tags_obj) = item.get("tags").and_then(|v| v.as_object()) {
            let mut tag_pairs: Vec<(&str, u64)> = tags_obj
                .iter()
                .map(|(k, v)| (k.as_str(), json_val_to_u64(Some(v))))
                .collect();
            tag_pairs.sort_by_key(|a| std::cmp::Reverse(a.1));
            for (tag_name, _) in tag_pairs.into_iter().take(12) {
                push_unique_label(&mut entry.tags, tag_name);
            }
        }

        if let Some(tag_label) = forced_tag {
            push_unique_label(&mut entry.tags, tag_label);
        }
        if let Some(genre_label) = mark_genre {
            push_unique_label(&mut entry.genres, genre_label);
        }
        if mark_sp {
            entry.is_singleplayer = true;
            push_unique_label(&mut entry.categories, "Single-player");
        }
        if mark_coop {
            entry.is_coop = true;
            push_unique_label(&mut entry.categories, "Co-op");
        }

        finalize_catalog_entry(entry, now);
    }
}

fn merge_featuredcategories(
    entries_by_id: &mut HashMap<u32, CatalogGameEntry>,
    root: &Value,
    now: u64,
) {
    for section_key in ["top_sellers", "new_releases", "specials", "coming_soon"] {
        let Some(items) = root
            .get(section_key)
            .and_then(|s| s.get("items"))
            .and_then(|i| i.as_array())
        else {
            continue;
        };

        for item in items {
            let app_id = json_val_to_u32(item.get("id"));
            if app_id == 0 {
                continue;
            }
            let name = item
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }

            let price_cents = json_val_to_u32(item.get("final_price"));
            let initial_cents = json_val_to_u32(item.get("original_price")).max(price_cents);
            let discount = json_val_to_u32(item.get("discount_percent"));

            let entry = entries_by_id.entry(app_id).or_insert_with(|| CatalogGameEntry {
                app_id,
                name: name.clone(),
                ..Default::default()
            });

            if !name.is_empty() {
                entry.name = name;
            }
            if !entry.details_enriched || price_cents > 0 {
                entry.price_cents = price_cents;
                entry.initial_price_cents = initial_cents;
                entry.discount_percent = discount;
                entry.price_formatted = if price_cents == 0 {
                    "Free".to_string()
                } else {
                    format!("${:.2}", price_cents as f64 / 100.0)
                };
            }
            if let Some(header) = item.get("large_capsule_image").and_then(|v| v.as_str()) {
                if !header.trim().is_empty() && entry.header_url.is_empty() {
                    entry.header_url = header.trim().to_string();
                }
            }
            push_unique_label(&mut entry.tags, "Featured");
            finalize_catalog_entry(entry, now);
        }
    }
}

fn apply_appdetails_and_players_to_entry(
    entry: &mut CatalogGameEntry,
    appid: u32,
    details_data: Option<&Value>,
    players_json: Option<&Value>,
    now: u64,
) {
    if let Some(data) = details_data {
        if let Some(name) = data.get("name").and_then(|v| v.as_str()) {
            if !name.trim().is_empty() {
                entry.name = name.trim().to_string();
            }
        }
        if let Some(desc) = data.get("short_description").and_then(|v| v.as_str()) {
            let clean_desc = strip_bbcode_and_html(desc);
            if !clean_desc.is_empty() {
                entry.short_description = clean_desc;
            }
        }
        if let Some(rel) = data
            .get("release_date")
            .and_then(|r| r.get("date"))
            .and_then(|d| d.as_str())
        {
            if !rel.trim().is_empty() {
                entry.release_date = rel.trim().to_string();
            }
        }
        if let Some(devs) = data.get("developers").and_then(|v| v.as_array()) {
            for d in devs.iter().filter_map(|x| x.as_str()) {
                push_unique_label(&mut entry.developers, d);
            }
        }
        if let Some(pubs) = data.get("publishers").and_then(|v| v.as_array()) {
            for p in pubs.iter().filter_map(|x| x.as_str()) {
                push_unique_label(&mut entry.publishers, p);
            }
        }
        if let Some(genres) = data.get("genres").and_then(|v| v.as_array()) {
            for g in genres
                .iter()
                .filter_map(|x| x.get("description").and_then(|d| d.as_str()))
            {
                push_unique_label(&mut entry.genres, g);
            }
        }
        if let Some(cats) = data.get("categories").and_then(|v| v.as_array()) {
            for c in cats
                .iter()
                .filter_map(|x| x.get("description").and_then(|d| d.as_str()))
            {
                push_unique_label(&mut entry.categories, c);
            }
        }
        if let Some(header) = data.get("header_image").and_then(|v| v.as_str()) {
            if !header.trim().is_empty() {
                entry.header_url = header.trim().to_string();
            }
        }

        let is_free = data
            .get("is_free")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if is_free {
            entry.price_cents = 0;
            entry.initial_price_cents = 0;
            entry.discount_percent = 0;
            entry.price_formatted = "Free".to_string();
            entry.is_free = true;
        } else if let Some(po) = data.get("price_overview") {
            let fin = json_val_to_u32(po.get("final"));
            let init = json_val_to_u32(po.get("initial")).max(fin);
            let disc = json_val_to_u32(po.get("discount_percent"));
            entry.price_cents = fin;
            entry.initial_price_cents = init;
            entry.discount_percent = disc;
            entry.price_formatted = po
                .get("final_formatted")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| format!("${:.2}", fin as f64 / 100.0));
            entry.is_free = false;
        }
        entry.details_enriched = true;
    }

    if let Some(count) = players_json
        .and_then(|p| p.get("response"))
        .and_then(|r| r.get("player_count"))
        .and_then(|c| c.as_u64())
    {
        entry.ccu = count;
    }

    entry.app_id = appid;
    entry.updated_at = now;
    finalize_catalog_entry(entry, now);
}

async fn enrich_catalog_entry_from_steam(
    appid: u32,
    existing: Option<CatalogGameEntry>,
    now: u64,
) -> Option<CatalogGameEntry> {
    if appid == 0 {
        return None;
    }
    let details_url = format!(
        "https://store.steampowered.com/api/appdetails?appids={}&cc=us&l=en",
        appid
    );
    let players_url = format!(
        "https://api.steampowered.com/ISteamUserStats/GetNumberOfCurrentPlayers/v1/?appid={}",
        appid
    );

    let (details_res, players_res) =
        tokio::join!(fetch_doh_json(&details_url), fetch_doh_json(&players_url));

    let details_json = details_res.ok();
    let players_json = players_res.ok();
    let details_data = details_json
        .as_ref()
        .and_then(|root| root.get(appid.to_string()))
        .and_then(|entry| {
            if entry.get("success").and_then(|v| v.as_bool()) == Some(false) {
                None
            } else {
                entry.get("data")
            }
        });

    let mut target = existing.unwrap_or_else(|| CatalogGameEntry {
        app_id: appid,
        ..Default::default()
    });
    if details_data.is_none() && players_json.is_none() && target.name.is_empty() {
        return None;
    }

    apply_appdetails_and_players_to_entry(
        &mut target,
        appid,
        details_data,
        players_json.as_ref(),
        now,
    );
    if target.name.is_empty() {
        return None;
    }
    Some(target)
}

const CATALOG_GENRE_QUERIES: [(&str, &str, bool); 21] = [
    ("Action", "Action", false),
    ("Adventure", "Adventure", false),
    ("RPG", "RPG", false),
    ("Simulation", "Simulation", false),
    ("Indie", "Indie", false),
    ("Racing", "Racing", false),
    ("Strategy", "Strategy", false),
    ("Engineering", "Engineering", true),
    ("Sandbox", "Sandbox", true),
    ("Automation", "Automation", true),
    ("Physics", "Physics", true),
    ("Space", "Space", true),
    ("Building", "Building", true),
    ("Survival", "Survival", true),
    ("Crafting", "Crafting", true),
    ("City+Builder", "City Builder", true),
    ("Open+World", "Open World", true),
    ("Horror", "Horror", true),
    ("Souls-like", "Souls-like", true),
    ("Metroidvania", "Metroidvania", true),
    ("Roguelike", "Roguelike", true),
];

struct CatalogSyncActiveGuard;

impl Drop for CatalogSyncActiveGuard {
    fn drop(&mut self) {
        if let Some(cell) = LIVE_CATALOG_UPSERTS.get() {
            if let Ok(mut guard) = cell.lock() {
                guard.clear();
            }
        }
        CATALOG_SYNC_ACTIVE.store(false, Ordering::SeqCst);
    }
}

fn record_live_catalog_upsert(entry: &CatalogGameEntry) {
    if CATALOG_SYNC_ACTIVE.load(Ordering::Acquire) {
        let cell = LIVE_CATALOG_UPSERTS.get_or_init(|| Mutex::new(HashMap::new()));
        if let Ok(mut guard) = cell.lock() {
            guard.insert(entry.app_id, entry.clone());
        }
    }
}

fn drain_live_upserts_into(entries_by_id: &mut HashMap<u32, CatalogGameEntry>) {
    if let Some(cell) = LIVE_CATALOG_UPSERTS.get() {
        if let Ok(mut guard) = cell.lock() {
            for (id, entry) in guard.drain() {
                entries_by_id.insert(id, entry);
            }
        }
    }
}

async fn fetch_doh_json_with_retry(
    raw_url: &str,
    max_retries: usize,
    jitter_seed: u64,
) -> Result<Value, String> {
    for attempt in 0..=max_retries {
        if attempt > 0 {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos() as u64;
            let backoff_ms = (650u64 << (attempt - 1)) + ((nanos ^ jitter_seed) % 220);
            tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
        }

        let client = build_metadata_doh_client(raw_url).await;
        match client
            .get(raw_url)
            .header(ACCEPT, "application/json, text/plain, */*")
            .timeout(Duration::from_secs(HTTP_METADATA_TIMEOUT_SECS))
            .send()
            .await
        {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    match resp.json::<Value>().await {
                        Ok(val) => return Ok(val),
                        Err(e) => {
                            if attempt == max_retries {
                                return Err(format!("Invalid JSON from {}: {}", raw_url, e));
                            }
                        }
                    }
                } else {
                    let code = status.as_u16();
                    let retryable = code == 429 || code == 408 || code >= 500;
                    if !retryable || attempt == max_retries {
                        return Err(format!("HTTP {} for {}", code, raw_url));
                    }
                }
            }
            Err(e) => {
                if attempt == max_retries {
                    return Err(format!("Request failed for {}: {}", raw_url, e));
                }
            }
        }
    }
    Err(format!("Exhausted retries for {}", raw_url))
}

#[derive(Clone, Copy)]
enum SteamSpyFetchTask {
    AllPage(u32),
    Featured,
    Tag {
        url_tag: &'static str,
        tag_label: &'static str,
        is_sp: bool,
        is_coop: bool,
        genre_label: Option<&'static str>,
    },
}

impl SteamSpyFetchTask {
    fn url(self) -> String {
        match self {
            Self::AllPage(page) => {
                format!("https://steamspy.com/api.php?request=all&page={}", page)
            }
            Self::Featured => {
                "https://store.steampowered.com/api/featuredcategories?cc=us&l=english".to_string()
            }
            Self::Tag { url_tag, .. } => {
                format!("https://steamspy.com/api.php?request=tag&tag={}", url_tag)
            }
        }
    }
}

async fn execute_steamspy_batch(
    semaphore: &Arc<Semaphore>,
    tasks: Vec<SteamSpyFetchTask>,
    entries_by_id: &mut HashMap<u32, CatalogGameEntry>,
    fetched_pages: &mut Vec<u32>,
    now: u64,
    polite_jitter: bool,
) -> (usize, Option<String>) {
    let mut handles = Vec::with_capacity(tasks.len());
    for (idx, task) in tasks.into_iter().enumerate() {
        let sem = semaphore.clone();
        handles.push(tokio::spawn(async move {
            let _permit = sem
                .acquire_owned()
                .await
                .map_err(|_| "SteamSpy semaphore closed".to_string())?;
            if polite_jitter {
                let nanos = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .subsec_nanos() as u64;
                let jitter_ms = 120 + (((idx as u64).wrapping_mul(31) ^ nanos) % 61);
                tokio::time::sleep(Duration::from_millis(jitter_ms)).await;
            }
            let url = task.url();
            let res = fetch_doh_json_with_retry(&url, 2, (idx as u64).wrapping_add(1)).await;
            Ok::<(SteamSpyFetchTask, Result<Value, String>), String>((task, res))
        }));
    }

    let mut successes = 0usize;
    let mut first_error: Option<String> = None;

    for handle in handles {
        let Ok(Ok((task, res))) = handle.await else {
            continue;
        };
        match res {
            Ok(val) => {
                match task {
                    SteamSpyFetchTask::AllPage(page) => {
                        merge_steamspy_map(entries_by_id, &val, None, false, false, None, now);
                        if !fetched_pages.contains(&page) {
                            fetched_pages.push(page);
                        }
                    }
                    SteamSpyFetchTask::Featured => {
                        merge_featuredcategories(entries_by_id, &val, now);
                    }
                    SteamSpyFetchTask::Tag {
                        tag_label,
                        is_sp,
                        is_coop,
                        genre_label,
                        ..
                    } => {
                        merge_steamspy_map(
                            entries_by_id,
                            &val,
                            Some(tag_label),
                            is_sp,
                            is_coop,
                            genre_label,
                            now,
                        );
                    }
                }
                successes += 1;
            }
            Err(err) => {
                if first_error.is_none() {
                    first_error = Some(err);
                }
            }
        }
    }

    (successes, first_error)
}

async fn flush_catalog_snapshot_async(
    app: &AppHandle,
    fetched_at: u64,
    fetched_pages: &[u32],
    entries_by_id: &mut HashMap<u32, CatalogGameEntry>,
) -> Result<usize, String> {
    drain_live_upserts_into(entries_by_id);

    let mut pages_sorted = fetched_pages.to_vec();
    pages_sorted.sort_unstable();
    pages_sorted.dedup();

    let mut snapshot: Vec<CatalogGameEntry> = entries_by_id.values().cloned().collect();
    snapshot.sort_by(|a, b| {
        b.value_score
            .partial_cmp(&a.value_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.ccu.cmp(&a.ccu))
            .then_with(|| b.owners_min.cmp(&a.owners_min))
    });
    let indexed_count = snapshot.len();

    let cache = CatalogCacheFile {
        fetched_at,
        fetched_pages: pages_sorted,
        entries: snapshot,
    };

    let app_for_save = app.clone();
    tokio::task::spawn_blocking(move || save_catalog_cache_to_disk(&app_for_save, &cache))
        .await
        .map_err(|e| format!("Catalog flush task panicked: {}", e))??;

    Ok(indexed_count)
}

fn emit_catalog_sync_progress(app: &AppHandle, indexed_count: usize, done: bool) {
    let payload = CatalogSyncProgressEvent {
        indexed_count,
        target_count: CATALOG_TARGET_COUNT.max(indexed_count),
        done,
    };
    let _ = app.emit("catalog-sync-progress", &payload);
}

struct BackgroundHarvestJob {
    app: AppHandle,
    semaphore: Arc<Semaphore>,
    entries_by_id: HashMap<u32, CatalogGameEntry>,
    fetched_pages: Vec<u32>,
    missing_pages: Vec<u32>,
    include_genres: bool,
    fetched_at: u64,
    now: u64,
}

async fn run_catalog_background_harvest(job: BackgroundHarvestJob) {
    let BackgroundHarvestJob {
        app,
        semaphore,
        mut entries_by_id,
        mut fetched_pages,
        missing_pages,
        include_genres,
        fetched_at,
        now,
    } = job;
    let _guard = CatalogSyncActiveGuard;

    let mut genre_idx = 0usize;
    let total_chunks = missing_pages.chunks(5).len();

    for (chunk_idx, page_chunk) in missing_pages.chunks(5).enumerate() {
        let mut tasks: Vec<SteamSpyFetchTask> = page_chunk
            .iter()
            .copied()
            .map(SteamSpyFetchTask::AllPage)
            .collect();

        if include_genres && genre_idx < CATALOG_GENRE_QUERIES.len() {
            let take_genres = if chunk_idx + 1 == total_chunks {
                CATALOG_GENRE_QUERIES.len() - genre_idx
            } else {
                4.min(CATALOG_GENRE_QUERIES.len() - genre_idx)
            };
            for &(q_tag, display_genre, is_tag_only) in
                &CATALOG_GENRE_QUERIES[genre_idx..genre_idx + take_genres]
            {
                tasks.push(SteamSpyFetchTask::Tag {
                    url_tag: q_tag,
                    tag_label: display_genre,
                    is_sp: false,
                    is_coop: false,
                    genre_label: if is_tag_only {
                        None
                    } else {
                        Some(display_genre)
                    },
                });
            }
            genre_idx += take_genres;
        }

        let (successes, _) = execute_steamspy_batch(
            &semaphore,
            tasks,
            &mut entries_by_id,
            &mut fetched_pages,
            now,
            true,
        )
        .await;

        let is_last_chunk = chunk_idx + 1 == total_chunks && (!include_genres || genre_idx >= CATALOG_GENRE_QUERIES.len());
        if successes > 0 || is_last_chunk {
            if let Ok(count) = flush_catalog_snapshot_async(
                &app,
                fetched_at,
                &fetched_pages,
                &mut entries_by_id,
            )
            .await
            {
                emit_catalog_sync_progress(&app, count, is_last_chunk);
            }
        }
    }

    if include_genres && genre_idx < CATALOG_GENRE_QUERIES.len() {
        let rem_tasks: Vec<SteamSpyFetchTask> = CATALOG_GENRE_QUERIES[genre_idx..]
            .iter()
            .map(|&(q_tag, display_genre, is_tag_only)| SteamSpyFetchTask::Tag {
                url_tag: q_tag,
                tag_label: display_genre,
                is_sp: false,
                is_coop: false,
                genre_label: if is_tag_only {
                    None
                } else {
                    Some(display_genre)
                },
            })
            .collect();
        let _ = execute_steamspy_batch(
            &semaphore,
            rem_tasks,
            &mut entries_by_id,
            &mut fetched_pages,
            now,
            true,
        )
        .await;
        if let Ok(count) =
            flush_catalog_snapshot_async(&app, fetched_at, &fetched_pages, &mut entries_by_id)
                .await
        {
            emit_catalog_sync_progress(&app, count, true);
        }
    } else if missing_pages.is_empty() {
        emit_catalog_sync_progress(&app, entries_by_id.len(), true);
    }

    drop(entries_by_id);
    drop(fetched_pages);
    trim_process_working_set();
}

#[tauri::command]
async fn fetch_catalog_games(
    app: AppHandle,
    page: Option<u32>,
    query: Option<String>,
    force_refresh: Option<bool>,
) -> Result<CatalogResponse, String> {
    let _ = (page, query);
    let force = force_refresh.unwrap_or(false);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let (total_indexed, fetched_at, mut fetched_pages) =
        tokio::task::spawn_blocking(catalog_mmap::catalog_mmap_header_meta)
            .await
            .map_err(|e| format!("Failed reading catalog metadata: {}", e))?;

    let cache_is_fresh = total_indexed > 0
        && fetched_at > 0
        && now.saturating_sub(fetched_at) < CATALOG_TTL_SECS;
    let missing_pages: Vec<u32> = (0..CATALOG_HARVEST_PAGES)
        .filter(|p| !fetched_pages.contains(p))
        .collect();
    let all_pages_present = missing_pages.is_empty();

    if cache_is_fresh && !force && all_pages_present {
        return Ok(CatalogResponse {
            games: Vec::new(),
            total_cached: total_indexed,
            fetched_at,
            from_cache: true,
            stale_warning: None,
            live_search_matched: 0,
        });
    }

    if CATALOG_SYNC_ACTIVE.load(Ordering::Acquire) {
        return Ok(CatalogResponse {
            games: Vec::new(),
            total_cached: total_indexed,
            fetched_at,
            from_cache: cache_is_fresh,
            stale_warning: None,
            live_search_matched: 0,
        });
    }

    // Cache is within 24h TTL with an initial batch already mapped, only missing pages remain.
    if cache_is_fresh && !force && !missing_pages.is_empty() {
        if CATALOG_SYNC_ACTIVE
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let app_load = app.clone();
            let existing = tokio::task::spawn_blocking(move || load_catalog_cache_from_disk(&app_load))
                .await
                .unwrap_or_default();
            let mut entries_by_id = HashMap::with_capacity(existing.entries.len().max(CATALOG_TARGET_COUNT));
            for mut item in existing.entries {
                finalize_catalog_entry(&mut item, now);
                if item.app_id > 0 && !item.name.is_empty() {
                    entries_by_id.insert(item.app_id, item);
                }
            }
            let semaphore = Arc::new(Semaphore::new(CATALOG_SEMAPHORE_CONCURRENCY));
            emit_catalog_sync_progress(&app, entries_by_id.len(), false);
            tokio::spawn(run_catalog_background_harvest(BackgroundHarvestJob {
                app: app.clone(),
                semaphore,
                entries_by_id,
                fetched_pages,
                missing_pages,
                include_genres: true,
                fetched_at,
                now,
            }));
        }
        return Ok(CatalogResponse {
            games: Vec::new(),
            total_cached: total_indexed,
            fetched_at,
            from_cache: true,
            stale_warning: None,
            live_search_matched: 0,
        });
    }

    // Immediate Stage (<2s): harvest pages 0..=2 + featured + core gameplay tags, flush & return.
    let mut entries_by_id: HashMap<u32, CatalogGameEntry> = if total_indexed > 0 {
        let app_load = app.clone();
        let existing = tokio::task::spawn_blocking(move || load_catalog_cache_from_disk(&app_load))
            .await
            .unwrap_or_default();
        let mut map = HashMap::with_capacity(existing.entries.len().max(4096));
        for mut item in existing.entries {
            finalize_catalog_entry(&mut item, now);
            if item.app_id > 0 && !item.name.is_empty() {
                map.insert(item.app_id, item);
            }
        }
        map
    } else {
        HashMap::with_capacity(4096)
    };

    if force {
        fetched_pages.clear();
    }

    let semaphore = Arc::new(Semaphore::new(CATALOG_SEMAPHORE_CONCURRENCY));
    let immediate_tasks = vec![
        SteamSpyFetchTask::AllPage(0),
        SteamSpyFetchTask::AllPage(1),
        SteamSpyFetchTask::AllPage(2),
        SteamSpyFetchTask::Featured,
        SteamSpyFetchTask::Tag {
            url_tag: "Singleplayer",
            tag_label: "Singleplayer",
            is_sp: true,
            is_coop: false,
            genre_label: None,
        },
        SteamSpyFetchTask::Tag {
            url_tag: "Co-op",
            tag_label: "Co-op",
            is_sp: false,
            is_coop: true,
            genre_label: None,
        },
        SteamSpyFetchTask::Tag {
            url_tag: "Massively+Multiplayer",
            tag_label: "Massively Multiplayer",
            is_sp: false,
            is_coop: false,
            genre_label: Some("Massively Multiplayer"),
        },
    ];

    let (successes, warn) = execute_steamspy_batch(
        &semaphore,
        immediate_tasks,
        &mut entries_by_id,
        &mut fetched_pages,
        now,
        true,
    )
    .await;

    if successes == 0 {
        if !entries_by_id.is_empty() {
            let total_cached = entries_by_id.len();
            drop(entries_by_id);
            trim_process_working_set();
            return Ok(CatalogResponse {
                games: Vec::new(),
                total_cached,
                fetched_at,
                from_cache: true,
                stale_warning: Some(format!(
                    "Showing cached catalog (live refresh failed: {})",
                    warn.unwrap_or_else(|| "network timeout".to_string())
                )),
                live_search_matched: 0,
            });
        }
        return Err(warn.unwrap_or_else(|| {
            "Failed to fetch Steam catalog from SteamSpy/Steam Store.".to_string()
        }));
    }

    let updated_fetched_at = now;
    let initial_count = flush_catalog_snapshot_async(
        &app,
        updated_fetched_at,
        &fetched_pages,
        &mut entries_by_id,
    )
    .await?;

    let remaining_pages: Vec<u32> = (3..CATALOG_HARVEST_PAGES)
        .filter(|p| force || !fetched_pages.contains(p))
        .collect();

    if remaining_pages.is_empty() {
        emit_catalog_sync_progress(&app, initial_count, true);
        drop(entries_by_id);
        trim_process_working_set();
    } else if CATALOG_SYNC_ACTIVE
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        emit_catalog_sync_progress(&app, initial_count, false);
        tokio::spawn(run_catalog_background_harvest(BackgroundHarvestJob {
            app: app.clone(),
            semaphore,
            entries_by_id,
            fetched_pages,
            missing_pages: remaining_pages,
            include_genres: true,
            fetched_at: updated_fetched_at,
            now,
        }));
    } else {
        drop(entries_by_id);
        trim_process_working_set();
    }

    Ok(CatalogResponse {
        games: Vec::new(),
        total_cached: initial_count,
        fetched_at: updated_fetched_at,
        from_cache: false,
        stale_warning: None,
        live_search_matched: 0,
    })
}

#[tauri::command]
async fn enrich_catalog_query(
    app: AppHandle,
    query: String,
) -> Result<CatalogEnrichResult, String> {
    let trimmed = query.trim().to_string();
    if trimmed.chars().count() < 2 {
        let (total_indexed, _, _) =
            tokio::task::spawn_blocking(catalog_mmap::catalog_mmap_header_meta)
                .await
                .unwrap_or((0, 0, Vec::new()));
        return Ok(CatalogEnrichResult {
            matched_count: 0,
            upserted_count: 0,
            total_indexed,
        });
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let encoded = urlencoding::encode(&trimmed);
    let search_url = format!(
        "https://store.steampowered.com/api/storesearch/?term={}&l=english&cc=US",
        encoded
    );

    let search_json = fetch_doh_json_with_retry(&search_url, 1, 17).await?;
    let items = search_json
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let candidates: Vec<(u32, String, u32)> = items
        .iter()
        .take(10)
        .filter_map(|item| {
            let id = json_val_to_u32(item.get("id"));
            let name = item.get("name").and_then(|v| v.as_str())?.trim().to_string();
            let price_cents = item
                .get("price")
                .map(|p| json_val_to_u32(p.get("final")))
                .unwrap_or(0);
            if id > 0 && !name.is_empty() {
                Some((id, name, price_cents))
            } else {
                None
            }
        })
        .collect();

    let matched_count = candidates.len();
    if candidates.is_empty() {
        let (total_indexed, _, _) =
            tokio::task::spawn_blocking(catalog_mmap::catalog_mmap_header_meta)
                .await
                .unwrap_or((0, 0, Vec::new()));
        return Ok(CatalogEnrichResult {
            matched_count: 0,
            upserted_count: 0,
            total_indexed,
        });
    }

    let cids: Vec<u32> = candidates.iter().map(|(id, _, _)| *id).collect();
    let existing_map: HashMap<u32, CatalogGameEntry> = tokio::task::spawn_blocking(move || {
        let mut map = HashMap::new();
        for cid in cids {
            if let Some(entry) = catalog_mmap::get_entry_via_mmap(cid) {
                map.insert(cid, entry);
            }
        }
        map
    })
    .await
    .unwrap_or_default();

    let mut enrich_targets: Vec<(u32, Option<CatalogGameEntry>)> = Vec::new();
    for (cid, cname, cprice) in candidates {
        let existing_in_mmap = existing_map.get(&cid);
        let base_opt = existing_in_mmap.cloned().or_else(|| {
            let mut stub = CatalogGameEntry {
                app_id: cid,
                name: cname,
                price_cents: cprice,
                initial_price_cents: cprice,
                ..Default::default()
            };
            finalize_catalog_entry(&mut stub, now);
            Some(stub)
        });
        let needs_enrich = existing_in_mmap
            .map(|e| !e.details_enriched || e.categories.is_empty())
            .unwrap_or(true);
        if needs_enrich {
            enrich_targets.push((cid, base_opt));
        }
    }

    let sem = Arc::new(Semaphore::new(CATALOG_SEMAPHORE_CONCURRENCY));
    let mut handles = Vec::with_capacity(enrich_targets.len());
    for (cid, base_opt) in enrich_targets {
        let sem_c = sem.clone();
        handles.push(tokio::spawn(async move {
            let _permit = sem_c.acquire_owned().await.ok();
            enrich_catalog_entry_from_steam(cid, base_opt.clone(), now)
                .await
                .or(base_opt)
        }));
    }

    let mut upserts: Vec<CatalogGameEntry> = Vec::new();
    for handle in handles {
        if let Ok(Some(resolved)) = handle.await {
            record_live_catalog_upsert(&resolved);
            upserts.push(resolved);
        }
    }

    let upserted_count = upserts.len();
    let total_indexed = if !upserts.is_empty() {
        let app_for_save = app.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = get_catalog_io_lock().lock().unwrap_or_else(|e| e.into_inner());
            let dkcat_path = get_app_data_file(&app_for_save, "catalog.dkcat")?;
            let stats = catalog_mmap::upsert_entries_dkcat(&dkcat_path, &upserts, None, &[])?;
            Ok::<usize, String>(stats.record_count as usize)
        })
        .await
        .map_err(|e| format!("Failed saving Steam search upserts: {}", e))??
    } else {
        tokio::task::spawn_blocking(catalog_mmap::catalog_mmap_header_meta)
            .await
            .map(|m| m.0)
            .unwrap_or(0)
    };

    trim_process_working_set();

    Ok(CatalogEnrichResult {
        matched_count,
        upserted_count,
        total_indexed,
    })
}

#[tauri::command]
async fn get_catalog_game_details(
    app: AppHandle,
    app_id: u32,
) -> Result<CatalogGameEntry, String> {
    if app_id == 0 {
        return Err("Invalid Steam App ID.".to_string());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let existing = tokio::task::spawn_blocking(move || catalog_mmap::get_entry_via_mmap(app_id))
        .await
        .map_err(|e| format!("Failed reading catalog entry: {}", e))?;

    if let Some(ref item) = existing {
        if item.platform_id > 0 || platforms::is_synthetic_retro_appid(app_id) {
            let mut retro_item = item.clone();
            let plat = platforms::platform_by_id(retro_item.platform_id);
            if retro_item.platform_name.is_empty() {
                retro_item.platform_name = plat.display_name.to_string();
            }
            if retro_item.platform_slug.is_empty() {
                retro_item.platform_slug = plat.slug.to_string();
            }
            retro_item.console_group = plat.console_group;
            if retro_item.cover_url.is_empty() {
                let (boxart_url, _, _) = retro_catalog::build_libretro_thumbnail_urls(
                    retro_item.platform_id,
                    &retro_item.name,
                );
                if !boxart_url.is_empty() {
                    retro_item.cover_url = boxart_url;
                }
            }
            if retro_item.header_url.is_empty() {
                retro_item.header_url = retro_item.cover_url.clone();
            }
            return Ok(retro_item);
        }
        if item.details_enriched
            && !item.short_description.is_empty()
            && now.saturating_sub(item.updated_at) < 3600
        {
            return Ok(item.clone());
        }
    }

    let enriched = enrich_catalog_entry_from_steam(app_id, existing.clone(), now)
        .await
        .or(existing)
        .ok_or_else(|| format!("Unable to load Steam store details for AppID {}.", app_id))?;

    record_live_catalog_upsert(&enriched);
    let enriched_for_cache = enriched.clone();
    let app_for_save = app.clone();
    let _ = tokio::task::spawn_blocking(move || {
        let _guard = get_catalog_io_lock().lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(dkcat_path) = get_app_data_file(&app_for_save, "catalog.dkcat") {
            let _ = catalog_mmap::upsert_entries_dkcat(
                &dkcat_path,
                &[enriched_for_cache],
                None,
                &[],
            );
        }
    })
    .await;

    trim_process_working_set();

    Ok(enriched)
}

#[tauri::command]
async fn get_local_library(app: AppHandle) -> Result<Vec<GameEntry>, String> {
    let app_for_load = app.clone();
    let entries = tokio::task::spawn_blocking(move || load_library_from_disk(&app_for_load))
        .await
        .map_err(|e| format!("Failed to load local library: {}", e))??;

    if entries.iter().any(|g| g.steam_price_cents.is_none()) {
        let app_bg = app.clone();
        tokio::spawn(async move {
            let _ = sync_library_steam_prices(app_bg).await;
        });
    }

    Ok(entries)
}

#[tauri::command]
async fn remove_local_game(app: AppHandle, id: String) -> Result<Vec<GameEntry>, String> {
    tokio::task::spawn_blocking(move || {
        let mut library = load_library_from_disk(&app)?;
        let removed_ids: Vec<String> = library
            .iter()
            .filter(|g| !(g.id != id && !g.exe_path.eq_ignore_ascii_case(&id)))
            .map(|g| g.id.clone())
            .collect();
        library.retain(|g| g.id != id && !g.exe_path.eq_ignore_ascii_case(&id));
        save_library_to_disk(&app, &library)?;
        sidebar_icons::invalidate_sidebar_icon(&app, &id);
        for gid in removed_ids {
            sidebar_icons::invalidate_sidebar_icon(&app, &gid);
        }
        Ok(library)
    })
    .await
    .map_err(|e| format!("Failed to remove game from library: {}", e))?
}

#[tauri::command]
async fn toggle_favorite_game(app: AppHandle, id: String) -> Result<Vec<GameEntry>, String> {
    tokio::task::spawn_blocking(move || {
        let mut library = load_library_from_disk(&app)?;
        if let Some(entry) = library
            .iter_mut()
            .find(|g| g.id == id || g.exe_path.eq_ignore_ascii_case(&id))
        {
            entry.is_favorite = !entry.is_favorite;
        }
        save_library_to_disk(&app, &library)?;
        Ok(library)
    })
    .await
    .map_err(|e| format!("Failed to toggle favorite state: {}", e))?
}

#[tauri::command]
async fn open_game_folder(path: String) -> Result<(), String> {
    let target = PathBuf::from(&path);
    let dir = if target.is_file() {
        target
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| "Could not resolve parent folder.".to_string())?
    } else {
        target
    };

    if !dir.exists() {
        return Err(format!("Folder does not exist: {}", dir.display()));
    }

    Command::new("explorer")
        .arg(dir)
        .spawn()
        .map_err(|e| format!("Failed to open Windows Explorer: {}", e))?;

    Ok(())
}

#[tauri::command]
async fn trim_working_set(app: AppHandle) -> Result<Value, String> {
    trim_process_working_set();
    let ram_mb = get_process_working_set_mb();
    publish_shm_event(
        ShmEventType::RamPurge,
        std::process::id(),
        ram_mb.to_bits(),
        0,
    );
    let doh_hosts = duke_core::net_doh::cached_doh_host_count();
    let payload = serde_json::json!({
        "workingSetMb": ram_mb,
        "dohShield": "ACTIVE",
        "dohCachedHosts": doh_hosts,
        "portableMode": resolve_portable_data_dir().is_some(),
        "status": "IDLE TRIMMED"
    });
    let _ = app.emit("memory-telemetry", &payload);
    Ok(payload)
}

#[tauri::command]
async fn get_system_telemetry() -> Result<Value, String> {
    let ram_mb = get_process_working_set_mb();
    let doh_hosts = duke_core::net_doh::cached_doh_host_count();
    Ok(serde_json::json!({
        "workingSetMb": ram_mb,
        "dohShield": "ACTIVE",
        "dohCachedHosts": doh_hosts,
        "portableMode": resolve_portable_data_dir().is_some(),
        "status": "IDLE TRIMMED"
    }))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedShortcut {
    pub target_exe: String,
    pub arguments: Option<String>,
    pub working_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestPathsReport {
    pub added_count: usize,
    pub skipped_count: usize,
    pub added_titles: Vec<String>,
    pub skipped_reasons: Vec<String>,
    pub library: Vec<GameEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopCleanupReport {
    pub scanned_shortcuts: usize,
    pub imported_count: usize,
    pub moved_to_backup: usize,
    pub already_in_library: usize,
    pub skipped_non_game: usize,
    pub backup_dir: String,
    pub imported_titles: Vec<String>,
    pub library: Vec<GameEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoFixReport {
    pub game_id: String,
    pub title: String,
    pub unblocked_files: usize,
    pub missing_dlls_found: usize,
    pub healed_dlls: Vec<String>,
    pub unresolved_dlls: Vec<String>,
    pub sandbox_path: String,
    pub sandbox_enabled: bool,
    pub summary: String,
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

fn expand_windows_env_placeholders(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' {
            if let Some(rel_end) = chars[i + 1..].iter().position(|&c| c == '%') {
                if rel_end > 0 {
                    let var_name: String = chars[i + 1..i + 1 + rel_end].iter().collect();
                    if let Ok(val) = std::env::var(&var_name) {
                        out.push_str(&val);
                        i += rel_end + 2;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn wide_buf_to_trimmed_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&ch| ch == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end]).trim().to_string()
}

/// Resolves a Windows `.lnk` shortcut's target executable, arguments, and working directory
/// using Win32 COM `IShellLinkW` + `IPersistFile`.
#[cfg(target_os = "windows")]
pub(crate) fn resolve_lnk_shortcut(lnk_path: &Path) -> Result<ResolvedShortcut, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED, STGM,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink, SLGP_RAWPATH};

    if !lnk_path.is_file() {
        return Err(format!("Shortcut file does not exist: {}", lnk_path.display()));
    }

    struct ComInitGuard(bool);
    impl Drop for ComInitGuard {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let _com_guard = ComInitGuard(hr.is_ok());

    let shell_link: IShellLinkW = unsafe {
        CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| format!("Failed creating IShellLinkW COM instance: {}", e))?
    };
    let persist_file: IPersistFile = shell_link
        .cast()
        .map_err(|e| format!("Failed querying IPersistFile interface: {}", e))?;

    let wide_lnk: Vec<u16> = lnk_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        persist_file
            .Load(PCWSTR(wide_lnk.as_ptr()), STGM(0))
            .map_err(|e| format!("Failed loading shortcut '{}': {}", lnk_path.display(), e))?;
    }

    let mut target_buf = vec![0u16; 1024];
    let _ = unsafe { shell_link.GetPath(&mut target_buf, std::ptr::null_mut(), 0) };
    let mut target_str = wide_buf_to_trimmed_string(&target_buf);
    if target_str.is_empty() {
        let mut raw_buf = vec![0u16; 1024];
        let _ = unsafe { shell_link.GetPath(&mut raw_buf, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32) };
        target_str = wide_buf_to_trimmed_string(&raw_buf);
    }
    let expanded_target = expand_windows_env_placeholders(&target_str)
        .trim_matches('"')
        .to_string();

    let mut args_buf = vec![0u16; 1024];
    let _ = unsafe { shell_link.GetArguments(&mut args_buf) };
    let args_str = wide_buf_to_trimmed_string(&args_buf);

    let mut work_buf = vec![0u16; 1024];
    let _ = unsafe { shell_link.GetWorkingDirectory(&mut work_buf) };
    let work_str = expand_windows_env_placeholders(&wide_buf_to_trimmed_string(&work_buf));

    if expanded_target.is_empty() {
        return Err(format!("Shortcut '{}' has no executable target path.", lnk_path.display()));
    }

    Ok(ResolvedShortcut {
        target_exe: expanded_target,
        arguments: (!args_str.is_empty()).then_some(args_str),
        working_dir: (!work_str.is_empty()).then_some(work_str),
    })
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn resolve_lnk_shortcut(_lnk_path: &Path) -> Result<ResolvedShortcut, String> {
    Err("Windows .lnk shortcut resolution is only supported on Windows.".to_string())
}

fn read_steam_appid_from_folder(folder: &Path) -> Option<u32> {
    for candidate in [
        folder.join("steam_appid.txt"),
        folder.join("Engine").join("Binaries").join("Win64").join("steam_appid.txt"),
    ] {
        if let Ok(raw) = fs::read_to_string(&candidate) {
            if let Ok(id) = raw.trim().parse::<u32>() {
                if id > 0 {
                    return Some(id);
                }
            }
        }
    }
    None
}

fn match_catalog_appid_for_title(title: &str) -> Option<u32> {
    let cleaned = clean_game_title(title);
    if cleaned.len() < 3 || cleaned.eq_ignore_ascii_case("Unknown Game") {
        return None;
    }
    let matches = catalog_mmap::search_catalog_via_mmap(&cleaned, 3).unwrap_or_default();
    let cleaned_lower = cleaned.to_lowercase();
    matches
        .into_iter()
        .find(|entry| {
            let entry_clean = clean_game_title(&entry.name).to_lowercase();
            entry_clean == cleaned_lower || entry_clean.contains(&cleaned_lower)
        })
        .map(|entry| entry.app_id)
}

async fn apply_shortcut_launch_args_if_empty(
    app: &AppHandle,
    game_id: &str,
    shortcut_args: Option<String>,
) {
    let Some(args) = shortcut_args.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) else {
        return;
    };
    let app_clone = app.clone();
    let gid_owned = game_id.to_string();
    let _ = tokio::task::spawn_blocking(move || {
        if let Ok(mut library) = load_library_from_disk(&app_clone) {
            if let Some(item) = library.iter_mut().find(|g| g.id == gid_owned) {
                if item.launch_args.as_deref().map(str::trim).unwrap_or("").is_empty() {
                    item.launch_args = Some(args);
                    let _ = save_library_to_disk(&app_clone, &library);
                }
            }
        }
    })
    .await;
}

/// Automatically ingests any dropped folders, `.exe` files, or `.lnk` shortcuts in zero extra clicks.
type CoverCacheTarget = (u16, String, String, Option<String>);

struct RetroUpsertParams<'a> {
    rom_path: Option<&'a Path>,
    inspection: Option<&'a rom_inspect::RomInspectionResult>,
    platform_id: u16,
    custom_title: Option<&'a str>,
    catalog_app_id: Option<u32>,
    cover_hint: Option<&'a str>,
    hero_hint: Option<&'a str>,
    origin_source: &'a str,
}

fn upsert_retro_game_entry_in_library(
    library: &mut Vec<GameEntry>,
    params: RetroUpsertParams<'_>,
) -> GameEntry {
    let rom_path = params.rom_path;
    let inspection = params.inspection;
    let platform_id = params.platform_id;
    let custom_title = params.custom_title;
    let catalog_app_id = params.catalog_app_id;
    let cover_hint = params.cover_hint;
    let hero_hint = params.hero_hint;
    let origin_source = params.origin_source;
    let eff_plat_id = inspection
        .map(|i| i.platform_id)
        .filter(|&id| id > 0)
        .unwrap_or(platform_id);
    let plat = platforms::platform_by_id(eff_plat_id);

    let rom_id = inspection
        .map(|i| i.rom_id.trim().to_string())
        .unwrap_or_default();
    let raw_title = custom_title
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            inspection
                .map(|i| i.detected_title.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            rom_path
                .and_then(|p| p.file_stem())
                .and_then(|s| s.to_str())
                .map(clean_game_title)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| format!("{} Title", plat.short_name));

    let catalog_match = catalog_app_id
        .and_then(catalog_mmap::get_entry_via_mmap)
        .or_else(|| {
            catalog_mmap::lookup_retro_entry_by_title_or_id(eff_plat_id, &rom_id, &raw_title)
        });

    let resolved_title = if custom_title.is_some() {
        raw_title.clone()
    } else if let Some(ref cat) = catalog_match {
        let base = retro_catalog::normalize_retro_base_title(&cat.name);
        if !base.is_empty() {
            base
        } else {
            raw_title.clone()
        }
    } else {
        raw_title.clone()
    };

    let resolved_app_id = catalog_app_id.or_else(|| catalog_match.as_ref().map(|c| c.app_id));
    let resolved_cover = cover_hint
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            catalog_match
                .as_ref()
                .map(|c| c.cover_url.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            let (boxart_url, _, _) =
                retro_catalog::build_libretro_thumbnail_urls(eff_plat_id, &resolved_title);
            if boxart_url.is_empty() {
                None
            } else {
                Some(boxart_url)
            }
        });
    let resolved_hero = hero_hint
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            catalog_match
                .as_ref()
                .map(|c| c.hero_url.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| resolved_cover.clone());

    let rom_path_str = rom_path
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (install_dir_str, size_bytes) = if let Some(p) = rom_path {
        let dir = p
            .parent()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default();
        let bytes = fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        (dir, bytes)
    } else {
        (String::new(), 0)
    };
    let is_installed = !rom_path_str.is_empty();
    let now_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if let Some(existing) = library.iter_mut().find(|g| {
        (!rom_path_str.is_empty() && g.exe_path.eq_ignore_ascii_case(&rom_path_str))
            || (g.platform_id == eff_plat_id
                && resolved_app_id.is_some()
                && g.app_id == resolved_app_id)
    }) {
        if is_installed {
            existing.exe_path = rom_path_str.clone();
            existing.install_dir = install_dir_str;
            existing.size_bytes = size_bytes;
            existing.size_formatted = format_bytes(size_bytes);
            existing.is_installed = true;
        }
        existing.platform_id = eff_plat_id;
        existing.console_group = plat.console_group;
        existing.platform = plat.display_name.to_string();
        if existing.emulator_id.trim().is_empty() {
            existing.emulator_id = plat.default_emulator_id.to_string();
        }
        if !rom_id.is_empty() {
            existing.rom_id = rom_id;
        }
        if existing.cover_url.is_empty() {
            if let Some(ref c) = resolved_cover {
                existing.cover_url = c.clone();
            }
        }
        if existing.hero_url.is_none() {
            existing.hero_url = resolved_hero;
        }
        if existing.app_id.is_none() {
            existing.app_id = resolved_app_id;
        }
        return existing.clone();
    }

    let id = if !rom_path_str.is_empty() {
        format!(
            "retro_{}_{:08x}",
            plat.slug,
            crc32_ieee(rom_path_str.to_ascii_lowercase().as_bytes())
        )
    } else if let Some(aid) = resolved_app_id {
        format!("retro_{}_{}", plat.slug, aid)
    } else {
        format!(
            "retro_{}_{:08x}",
            plat.slug,
            crc32_ieee(resolved_title.to_ascii_lowercase().as_bytes())
        )
    };

    let entry = GameEntry {
        id,
        title: resolved_title,
        exe_path: rom_path_str,
        install_dir: install_dir_str,
        size_bytes,
        size_formatted: format_bytes(size_bytes),
        cover_url: resolved_cover.unwrap_or_default(),
        hero_url: resolved_hero,
        icon_path: None,
        genre: format!("{} Console", plat.short_name),
        platform: plat.display_name.to_string(),
        source: plat.display_name.to_string(),
        acquisition_source: origin_source.to_string(),
        is_favorite: false,
        is_installed,
        added_at: now_ts,
        app_id: resolved_app_id,
        steam_app_id: resolved_app_id.map(u64::from),
        platform_id: eff_plat_id,
        console_group: plat.console_group,
        rom_id,
        emulator_id: plat.default_emulator_id.to_string(),
        ..Default::default()
    };
    library.push(entry.clone());
    entry
}

fn spawn_cache_retro_covers_batch(app: AppHandle, targets: Vec<CoverCacheTarget>) {
    if targets.is_empty() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let sem = Arc::new(tokio::sync::Semaphore::new(4));
        let mut updates: Vec<(String, String, String)> = Vec::new();
        let mut join_set = tokio::task::JoinSet::new();
        for (plat_id, gid, title, hint) in targets.into_iter().take(64) {
            let permit = Arc::clone(&sem);
            join_set.spawn(async move {
                let Ok(_p) = permit.acquire().await else {
                    return None;
                };
                retro_catalog::cache_retro_cover_and_icon(
                    plat_id,
                    &gid,
                    &title,
                    hint.as_deref(),
                )
                .await
                .map(|(cov, ico)| (gid, cov, ico))
            });
        }
        while let Some(res) = join_set.join_next().await {
            if let Ok(Some(item)) = res {
                updates.push(item);
            }
        }
        if !updates.is_empty() {
            let app_save = app.clone();
            if let Ok(Ok(lib)) = tokio::task::spawn_blocking(move || -> Result<Vec<GameEntry>, String> {
                let mut lib = load_library_from_disk(&app_save)?;
                for (gid, cov, ico) in &updates {
                    if let Some(entry) = lib.iter_mut().find(|g| &g.id == gid) {
                        if !cov.is_empty() {
                            entry.cover_url = cov.clone();
                        }
                        if !ico.is_empty() {
                            entry.icon_path = Some(ico.clone());
                        }
                    }
                }
                save_library_to_disk(&app_save, &lib)?;
                Ok(lib)
            })
            .await
            {
                let _ = app.emit("library-updated", &lib);
            }
        }
    });
}

#[tauri::command]
async fn inspect_rom_path(path: String) -> Result<rom_inspect::RomInspectionResult, String> {
    let trimmed = path.trim().trim_matches('"').to_string();
    let p = PathBuf::from(&trimmed);
    if !p.is_file() {
        return Err(format!("ROM / ISO file not found: {}", trimmed));
    }
    tokio::task::spawn_blocking(move || {
        rom_inspect::inspect_rom_file(&p)
            .ok_or_else(|| "Unrecognized console ROM or disc image header.".to_string())
    })
    .await
    .map_err(|e| format!("ROM inspection worker failed: {}", e))?
}

#[tauri::command]
async fn add_retro_game_from_catalog(
    app: AppHandle,
    app_id: u32,
    rom_path: Option<String>,
) -> Result<GameEntry, String> {
    let cat_entry = tokio::task::spawn_blocking(move || catalog_mmap::get_entry_via_mmap(app_id))
        .await
        .map_err(|e| format!("Catalog lookup failed: {}", e))?
        .ok_or_else(|| format!("Catalog entry {} not found.", app_id))?;

    let trimmed_rom = rom_path
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty());
    let rom_buf = trimmed_rom.as_ref().map(PathBuf::from);
    if let Some(ref p) = rom_buf {
        if !p.is_file() {
            return Err(format!("ROM / ISO file not found: {}", p.display()));
        }
        unblock_ntfs_zone_identifier(p);
    }

    let rom_inspect_res = if let Some(ref p) = rom_buf {
        let p_clone = p.clone();
        tokio::task::spawn_blocking(move || rom_inspect::inspect_rom_file(&p_clone))
            .await
            .map_err(|e| format!("ROM inspection worker failed: {}", e))?
    } else {
        None
    };

    let app_clone = app.clone();
    let cat_clone = cat_entry.clone();
    let entry = tokio::task::spawn_blocking(move || -> Result<GameEntry, String> {
        let mut library = load_library_from_disk(&app_clone)?;
        let title_clean = retro_catalog::normalize_retro_base_title(&cat_clone.name);
        let eff_title = if title_clean.is_empty() {
            cat_clone.name.clone()
        } else {
            title_clean
        };
        let entry = upsert_retro_game_entry_in_library(
            &mut library,
            RetroUpsertParams {
                rom_path: rom_buf.as_deref(),
                inspection: rom_inspect_res.as_ref(),
                platform_id: cat_clone.platform_id,
                custom_title: Some(&eff_title),
                catalog_app_id: Some(cat_clone.app_id),
                cover_hint: if cat_clone.cover_url.is_empty() {
                    None
                } else {
                    Some(cat_clone.cover_url.as_str())
                },
                hero_hint: if cat_clone.hero_url.is_empty() {
                    None
                } else {
                    Some(cat_clone.hero_url.as_str())
                },
                origin_source: if rom_buf.is_some() {
                    "rom_linked"
                } else {
                    "catalog_retro"
                },
            },
        );
        save_library_to_disk(&app_clone, &library)?;
        let _ = app_clone.emit("library-updated", &library);
        Ok(entry)
    })
    .await
    .map_err(|e| format!("Library update worker failed: {}", e))??;

    let hint = if entry.cover_url.is_empty() {
        None
    } else {
        Some(entry.cover_url.clone())
    };
    spawn_cache_retro_covers_batch(
        app,
        vec![(entry.platform_id, entry.id.clone(), entry.title.clone(), hint)],
    );

    Ok(entry)
}

#[tauri::command]
async fn link_game_rom(
    app: AppHandle,
    game_id: String,
    rom_path: String,
) -> Result<Vec<GameEntry>, String> {
    let trimmed = rom_path.trim().trim_matches('"').to_string();
    let rom_buf = PathBuf::from(&trimmed);
    if !rom_buf.is_file() {
        return Err(format!("Selected ROM / ISO file does not exist: {}", trimmed));
    }
    unblock_ntfs_zone_identifier(&rom_buf);
    let rom_inspect_buf = rom_buf.clone();
    let inspection = tokio::task::spawn_blocking(move || rom_inspect::inspect_rom_file(&rom_inspect_buf))
        .await
        .map_err(|e| format!("ROM inspection worker failed: {}", e))?;

    let app_clone = app.clone();
    let updated = tokio::task::spawn_blocking(move || -> Result<Vec<GameEntry>, String> {
        let mut library = load_library_from_disk(&app_clone)?;
        let Some(entry) = library.iter_mut().find(|g| g.id == game_id) else {
            return Err("Game entry not found in library.".to_string());
        };
        let size_bytes = fs::metadata(&rom_buf).map(|m| m.len()).unwrap_or(0);
        entry.exe_path = trimmed.clone();
        entry.install_dir = rom_buf
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        entry.size_bytes = size_bytes;
        entry.size_formatted = format_bytes(size_bytes);
        entry.is_installed = true;
        if let Some(ref info) = inspection {
            if entry.platform_id == 0 && info.platform_id > 0 {
                let plat = platforms::platform_by_id(info.platform_id);
                entry.platform_id = info.platform_id;
                entry.console_group = plat.console_group;
                entry.platform = plat.display_name.to_string();
                entry.emulator_id = plat.default_emulator_id.to_string();
            }
            if !info.rom_id.trim().is_empty() {
                entry.rom_id = info.rom_id.clone();
            }
        }
        save_library_to_disk(&app_clone, &library)?;
        let _ = app_clone.emit("library-updated", &library);
        Ok(library)
    })
    .await
    .map_err(|e| format!("Failed linking ROM: {}", e))??;

    Ok(updated)
}

#[tauri::command]
async fn scan_rom_folder(app: AppHandle, folder_path: String) -> Result<IngestPathsReport, String> {
    let trimmed = folder_path.trim().trim_matches('"').to_string();
    let dir = PathBuf::from(&trimmed);
    if !dir.is_dir() {
        return Err(format!("Selected ROM folder does not exist: {}", trimmed));
    }

    let dir_m3u = dir.clone();
    let detected = tokio::task::spawn_blocking(move || {
        let _ = rom_patcher::generate_m3u_playlists_in_dir(&dir_m3u);
        rom_inspect::scan_folder_for_roms(&dir, 4)
    })
    .await
    .map_err(|e| format!("ROM folder scanner failed: {}", e))?;

    if detected.is_empty() {
        let library = load_library_from_disk(&app).unwrap_or_default();
        return Ok(IngestPathsReport {
            added_count: 0,
            skipped_count: 1,
            added_titles: Vec::new(),
            skipped_reasons: vec![format!("No supported console ROM / ISO files found in '{}'.", trimmed)],
            library,
        });
    }

    let app_clone = app.clone();
    let (added_titles, cache_targets, library) = tokio::task::spawn_blocking(move || -> Result<_, String> {
        let mut lib = load_library_from_disk(&app_clone)?;
        let mut titles = Vec::new();
        let mut covers = Vec::new();
        for info in detected {
            let p = PathBuf::from(&info.path);
            unblock_ntfs_zone_identifier(&p);
            let entry = upsert_retro_game_entry_in_library(
                &mut lib,
                RetroUpsertParams {
                    rom_path: Some(&p),
                    inspection: Some(&info),
                    platform_id: info.platform_id,
                    custom_title: None,
                    catalog_app_id: None,
                    cover_hint: None,
                    hero_hint: None,
                    origin_source: "rom_scan",
                },
            );
            titles.push(entry.title.clone());
            let hint = if entry.cover_url.is_empty() {
                None
            } else {
                Some(entry.cover_url.clone())
            };
            covers.push((entry.platform_id, entry.id, entry.title, hint));
        }
        save_library_to_disk(&app_clone, &lib)?;
        let _ = app_clone.emit("library-updated", &lib);
        Ok((titles, covers, lib))
    })
    .await
    .map_err(|e| format!("Failed saving scanned ROMs: {}", e))??;

    spawn_cache_retro_covers_batch(app, cache_targets);

    Ok(IngestPathsReport {
        added_count: added_titles.len(),
        skipped_count: 0,
        added_titles,
        skipped_reasons: Vec::new(),
        library,
    })
}

pub(crate) async fn ingest_dropped_paths_core(
    app: AppHandle,
    paths: Vec<String>,
) -> Result<IngestPathsReport, String> {
    ingest_dropped_paths(app, paths).await
}

/// Canonicalizes a filesystem path while stripping Windows `\\?\` verbatim prefixes so duplicate
/// drops, symlinks, casing variations, and relative segments resolve to one stable key.
fn canonicalize_drop_path(path: &Path) -> PathBuf {
    let resolved = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    #[cfg(target_os = "windows")]
    {
        let s = resolved.to_string_lossy();
        if let Some(unc) = s.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{}", unc));
        }
        if let Some(plain) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(plain);
        }
    }
    resolved
}

#[tauri::command]
async fn ingest_dropped_paths(
    app: AppHandle,
    paths: Vec<String>,
) -> Result<IngestPathsReport, String> {
    let mut seen = HashSet::new();
    let mut added_titles = Vec::new();
    let mut skipped_reasons = Vec::new();

    for raw in paths {
        let trimmed = raw.trim().trim_matches('"').to_string();
        if trimmed.is_empty() {
            continue;
        }
        let raw_path = PathBuf::from(&trimmed);
        if !raw_path.exists() {
            skipped_reasons.push(format!("Skipped '{}': path no longer exists.", trimmed));
            continue;
        }
        let path = canonicalize_drop_path(&raw_path);
        let norm_key = path.to_string_lossy().to_lowercase();
        if !seen.insert(norm_key) {
            continue;
        }

        if path.is_dir() {
            unblock_ntfs_zone_identifier(&path);
            let folder_title = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(clean_game_title)
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| "Dropped Game".to_string());
            let dir_scan = path.clone();
            let title_scan = folder_title.clone();
            let best_exe = tokio::task::spawn_blocking(move || {
                find_best_game_exe_with_max_depth(&dir_scan, &title_scan, 3)
            })
            .await
            .map_err(|e| format!("Folder scan worker failed: {}", e))?;

            let Some(exe_path) = best_exe else {
                let dir_for_roms = path.clone();
                let scanned_roms = tokio::task::spawn_blocking(move || {
                    rom_inspect::scan_folder_for_roms(&dir_for_roms, 4)
                })
                .await
                .map_err(|e| format!("ROM scanner failed: {}", e))?;

                if scanned_roms.is_empty() {
                    skipped_reasons.push(format!(
                        "No playable game executable or console ROM found inside '{}'.",
                        folder_title
                    ));
                    continue;
                }

                struct ScannedRomBatch {
                    titles: Vec<String>,
                    covers: Vec<CoverCacheTarget>,
                }
                let app_for_roms = app.clone();
                let batch = tokio::task::spawn_blocking(move || -> Result<ScannedRomBatch, String> {
                    let mut lib = load_library_from_disk(&app_for_roms)?;
                    let mut titles = Vec::new();
                    let mut covers = Vec::new();
                    for info in scanned_roms {
                        let rom_p = PathBuf::from(&info.path);
                        unblock_ntfs_zone_identifier(&rom_p);
                        let entry = upsert_retro_game_entry_in_library(
                            &mut lib,
                            RetroUpsertParams {
                                rom_path: Some(&rom_p),
                                inspection: Some(&info),
                                platform_id: info.platform_id,
                                custom_title: None,
                                catalog_app_id: None,
                                cover_hint: None,
                                hero_hint: None,
                                origin_source: "dropped_rom_folder",
                            },
                        );
                        titles.push(entry.title.clone());
                        let hint = if entry.cover_url.is_empty() {
                            None
                        } else {
                            Some(entry.cover_url.clone())
                        };
                        covers.push((entry.platform_id, entry.id, entry.title, hint));
                    }
                    save_library_to_disk(&app_for_roms, &lib)?;
                    let _ = app_for_roms.emit("library-updated", &lib);
                    Ok(ScannedRomBatch { titles, covers })
                })
                .await
                .map_err(|e| format!("Failed saving dropped ROMs: {}", e))??;

                added_titles.extend(batch.titles);
                spawn_cache_retro_covers_batch(app.clone(), batch.covers);
                continue;
            };
            let app_id = read_steam_appid_from_folder(&path)
                .or_else(|| match_catalog_appid_for_title(&folder_title));
            match add_local_game_with_origin(
                app.clone(),
                exe_path.to_string_lossy().to_string(),
                Some(folder_title),
                None,
                None,
                app_id,
                Some("dropped_folder"),
            )
            .await
            {
                Ok(entry) => {
                    let _ = sidebar_icons::get_sidebar_icons(
                        app.clone(),
                        vec![entry.id.clone()],
                    )
                    .await;
                    added_titles.push(entry.title);
                }
                Err(err) => skipped_reasons.push(err),
            }
            continue;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        if ext == "dkpack" {
            match dkpack::ingest_dropped_dkpack_file(&app, &path, None).await {
                Ok(report) => {
                    let _ = app.emit("dkpack-applied", &report);
                    added_titles.push(format!("Modpack: {}", report.pack_name));
                }
                Err(err) => skipped_reasons.push(err),
            }
            continue;
        }

        if ext == "lnk" {
            let lnk_clone = path.clone();
            let resolved = tokio::task::spawn_blocking(move || resolve_lnk_shortcut(&lnk_clone))
                .await
                .map_err(|e| format!("Shortcut resolver failed: {}", e))?;
            let resolved = match resolved {
                Ok(r) => r,
                Err(err) => {
                    skipped_reasons.push(err);
                    continue;
                }
            };
            let target_path = PathBuf::from(&resolved.target_exe);
            if !target_path.is_file()
                || target_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| !e.eq_ignore_ascii_case("exe"))
                    .unwrap_or(true)
            {
                skipped_reasons.push(format!(
                    "Shortcut '{}' does not point to a valid .exe file.",
                    path.file_name().and_then(|s| s.to_str()).unwrap_or("shortcut")
                ));
                continue;
            }
            let game_root = resolve_game_root_dir(&target_path);
            let lnk_title = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(clean_game_title)
                .filter(|t| !t.is_empty() && t != "Unknown Game");
            let app_id = read_steam_appid_from_folder(&game_root)
                .or_else(|| lnk_title.as_deref().and_then(match_catalog_appid_for_title));

            match add_local_game_with_origin(
                app.clone(),
                resolved.target_exe,
                lnk_title,
                None,
                None,
                app_id,
                Some("dropped_shortcut"),
            )
            .await
            {
                Ok(entry) => {
                    apply_shortcut_launch_args_if_empty(&app, &entry.id, resolved.arguments).await;
                    let _ = sidebar_icons::get_sidebar_icons(
                        app.clone(),
                        vec![entry.id.clone()],
                    )
                    .await;
                    added_titles.push(entry.title);
                }
                Err(err) => skipped_reasons.push(err),
            }
            continue;
        }

        if ext == "exe" {
            let game_root = resolve_game_root_dir(&path);
            let fallback_name = game_root
                .file_name()
                .and_then(|n| n.to_str())
                .map(clean_game_title)
                .filter(|t| !t.is_empty() && t != "Unknown Game");
            let app_id = read_steam_appid_from_folder(&game_root)
                .or_else(|| fallback_name.as_deref().and_then(match_catalog_appid_for_title));

            match add_local_game_with_origin(
                app.clone(),
                path.to_string_lossy().to_string(),
                fallback_name,
                None,
                None,
                app_id,
                Some("dropped_exe"),
            )
            .await
            {
                Ok(entry) => {
                    let _ = sidebar_icons::get_sidebar_icons(
                        app.clone(),
                        vec![entry.id.clone()],
                    )
                    .await;
                    added_titles.push(entry.title);
                }
                Err(err) => skipped_reasons.push(err),
            }
            continue;
        }

        let path_for_rom = path.clone();
        if let Ok(Some(rom_info)) =
            tokio::task::spawn_blocking(move || rom_inspect::inspect_rom_file(&path_for_rom)).await
        {
            unblock_ntfs_zone_identifier(&path);
            let app_for_rom = app.clone();
            let path_owned = path.clone();
            let entry = tokio::task::spawn_blocking(move || -> Result<GameEntry, String> {
                let mut lib = load_library_from_disk(&app_for_rom)?;
                let entry = upsert_retro_game_entry_in_library(
                    &mut lib,
                    RetroUpsertParams {
                        rom_path: Some(&path_owned),
                        inspection: Some(&rom_info),
                        platform_id: rom_info.platform_id,
                        custom_title: None,
                        catalog_app_id: None,
                        cover_hint: None,
                        hero_hint: None,
                        origin_source: "dropped_rom",
                    },
                );
                save_library_to_disk(&app_for_rom, &lib)?;
                let _ = app_for_rom.emit("library-updated", &lib);
                Ok(entry)
            })
            .await
            .map_err(|e| format!("Failed adding dropped ROM: {}", e))??;

            added_titles.push(entry.title.clone());
            let hint = if entry.cover_url.is_empty() {
                None
            } else {
                Some(entry.cover_url.clone())
            };
            spawn_cache_retro_covers_batch(
                app.clone(),
                vec![(entry.platform_id, entry.id, entry.title, hint)],
            );
            continue;
        }

        skipped_reasons.push(format!(
            "Unsupported drop '{}': drop a game folder, .exe, .lnk shortcut, or console ROM / ISO.",
            path.file_name().and_then(|n| n.to_str()).unwrap_or(&trimmed)
        ));
    }

    let library = tokio::task::spawn_blocking({
        let app_clone = app.clone();
        move || load_library_from_disk(&app_clone)
    })
    .await
    .map_err(|e| format!("Library reload failed: {}", e))??;
    let _ = app.emit("library-updated", &library);

    Ok(IngestPathsReport {
        added_count: added_titles.len(),
        skipped_count: skipped_reasons.len(),
        added_titles,
        skipped_reasons,
        library,
    })
}

fn is_likely_non_game_shortcut(lnk_path: &Path, target_exe: &Path) -> bool {
    let lnk_stem = lnk_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if lnk_stem.contains("duke launcher")
        || lnk_stem.contains("duke systems")
        || lnk_stem.contains("uninstall")
    {
        return true;
    }

    let target_lower = target_exe.to_string_lossy().to_ascii_lowercase();
    if target_lower.contains("\\windows\\")
        || target_lower.contains("\\system32\\")
        || target_lower.contains("\\syswow64\\")
        || target_lower.contains("\\programdata\\microsoft\\")
    {
        return true;
    }

    let exe_name = target_exe
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    const NON_GAME_EXES: &[&str] = &[
        "duke-launcher.exe",
        "duke-systems.exe",
        "duke-monitor.exe",
        "duke-capture.exe",
        "duke-input.exe",
        "steam.exe",
        "epicgameslauncher.exe",
        "galaxyclient.exe",
        "upc.exe",
        "eadesktop.exe",
        "battle.net.exe",
        "chrome.exe",
        "msedge.exe",
        "firefox.exe",
        "brave.exe",
        "opera.exe",
        "discord.exe",
        "slack.exe",
        "spotify.exe",
        "code.exe",
        "devenv.exe",
        "obs64.exe",
        "obs32.exe",
        "vlc.exe",
        "7zfm.exe",
        "winrar.exe",
        "explorer.exe",
        "cmd.exe",
        "powershell.exe",
        "pwsh.exe",
        "wt.exe",
        "notepad.exe",
        "unins000.exe",
        "uninstall.exe",
        "setup.exe",
        "dxsetup.exe",
    ];
    if NON_GAME_EXES.contains(&exe_name.as_str()) {
        return true;
    }
    false
}

#[derive(Debug, Clone)]
struct DesktopShortcutCandidate {
    lnk_path: PathBuf,
    title: String,
    resolved: ResolvedShortcut,
}

fn move_file_with_unique_name(src: &Path, dest_dir: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(dest_dir)
        .map_err(|e| format!("Failed creating shortcut backup folder: {}", e))?;
    let stem = src
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Shortcut");
    let ext = src.extension().and_then(|s| s.to_str()).unwrap_or("lnk");
    let mut candidate = dest_dir.join(format!("{}.{}", stem, ext));
    let mut suffix = 1usize;
    while candidate.exists() {
        candidate = dest_dir.join(format!("{}_{}.{}", stem, suffix, ext));
        suffix += 1;
    }
    if fs::rename(src, &candidate).is_err() {
        fs::copy(src, &candidate)
            .map_err(|e| format!("Failed copying shortcut '{}': {}", src.display(), e))?;
        let _ = fs::remove_file(src);
    }
    Ok(candidate)
}

fn collect_desktop_game_shortcuts_sync(
    desktop_dirs: &[PathBuf],
) -> (usize, usize, Vec<DesktopShortcutCandidate>) {
    let mut scanned = 0usize;
    let mut skipped_non_game = 0usize;
    let mut candidates = Vec::new();

    for dir in desktop_dirs {
        if !dir.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let is_lnk = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("lnk"))
                .unwrap_or(false);
            if !is_lnk {
                continue;
            }
            scanned += 1;
            let Ok(resolved) = resolve_lnk_shortcut(&path) else {
                skipped_non_game += 1;
                continue;
            };
            let target_path = PathBuf::from(&resolved.target_exe);
            let is_exe = target_path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("exe"))
                .unwrap_or(false);
            if !target_path.is_file()
                || !is_exe
                || is_likely_non_game_shortcut(&path, &target_path)
            {
                skipped_non_game += 1;
                continue;
            }

            let title = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(clean_game_title)
                .filter(|t| !t.is_empty() && t != "Unknown Game")
                .unwrap_or_else(|| {
                    target_path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .map(clean_game_title)
                        .unwrap_or_else(|| "Game".to_string())
                });

            candidates.push(DesktopShortcutCandidate {
                lnk_path: path,
                title,
                resolved,
            });
        }
    }

    (scanned, skipped_non_game, candidates)
}

fn resolve_user_and_public_desktop_dirs() -> Result<(PathBuf, Vec<PathBuf>), String> {
    let mut dirs = Vec::new();
    let user_desktop = std::env::var("USERPROFILE")
        .ok()
        .map(|p| PathBuf::from(p).join("Desktop"))
        .filter(|p| !p.as_os_str().is_empty());

    let primary_desktop = user_desktop
        .clone()
        .ok_or_else(|| "Could not resolve USERPROFILE Desktop directory.".to_string())?;
    if primary_desktop.is_dir() {
        dirs.push(primary_desktop.clone());
    }

    if let Ok(public_root) = std::env::var("PUBLIC") {
        let public_desktop = PathBuf::from(public_root).join("Desktop");
        if public_desktop.is_dir()
            && !dirs
                .iter()
                .any(|d| d.to_string_lossy().eq_ignore_ascii_case(&public_desktop.to_string_lossy()))
        {
            dirs.push(public_desktop);
        }
    }

    Ok((primary_desktop.join("Duke Shortcut Backup"), dirs))
}

/// Scans `%USERPROFILE%\Desktop` and Public Desktop for `.lnk` game shortcuts using Win32
/// `IShellLinkW`, imports them into `library.json`, and moves processed `.lnk` files into
/// `%USERPROFILE%\Desktop\Duke Shortcut Backup\`.
#[tauri::command]
async fn scan_and_clean_desktop_shortcuts(app: AppHandle) -> Result<DesktopCleanupReport, String> {
    let (backup_dir, desktop_dirs) = resolve_user_and_public_desktop_dirs()?;
    let dirs_for_scan = desktop_dirs.clone();
    let (scanned_shortcuts, skipped_non_game, candidates) =
        tokio::task::spawn_blocking(move || collect_desktop_game_shortcuts_sync(&dirs_for_scan))
            .await
            .map_err(|e| format!("Desktop shortcut scanner failed: {}", e))?;

    let existing_library = tokio::task::spawn_blocking({
        let app_clone = app.clone();
        move || load_library_from_disk(&app_clone)
    })
    .await
    .map_err(|e| format!("Library read failed: {}", e))??;

    let existing_paths: HashSet<String> = existing_library
        .iter()
        .map(|g| g.exe_path.trim().to_ascii_lowercase())
        .collect();

    let mut imported_count = 0usize;
    let mut moved_to_backup = 0usize;
    let mut already_in_library = 0usize;
    let mut imported_titles = Vec::new();

    for cand in candidates {
        let exe_key = cand.resolved.target_exe.trim().to_ascii_lowercase();
        if existing_paths.contains(&exe_key) {
            already_in_library += 1;
            if move_file_with_unique_name(&cand.lnk_path, &backup_dir).is_ok() {
                moved_to_backup += 1;
            }
            continue;
        }

        let target_exe = PathBuf::from(&cand.resolved.target_exe);
        let game_root = resolve_game_root_dir(&target_exe);
        let app_id = read_steam_appid_from_folder(&game_root)
            .or_else(|| match_catalog_appid_for_title(&cand.title));

        if let Ok(entry) = add_local_game_with_origin(
            app.clone(),
            cand.resolved.target_exe.clone(),
            Some(cand.title.clone()),
            None,
            None,
            app_id,
            Some("desktop_shortcut"),
        )
        .await
        {
            apply_shortcut_launch_args_if_empty(&app, &entry.id, cand.resolved.arguments.clone())
                .await;
            let _ = sidebar_icons::get_sidebar_icons(app.clone(), vec![entry.id.clone()]).await;
            imported_count += 1;
            imported_titles.push(entry.title);
            if move_file_with_unique_name(&cand.lnk_path, &backup_dir).is_ok() {
                moved_to_backup += 1;
            }
        }
    }

    let library = tokio::task::spawn_blocking({
        let app_clone = app.clone();
        move || load_library_from_disk(&app_clone)
    })
    .await
    .map_err(|e| format!("Library reload failed: {}", e))??;
    let _ = app.emit("library-updated", &library);

    Ok(DesktopCleanupReport {
        scanned_shortcuts,
        imported_count,
        moved_to_backup,
        already_in_library,
        skipped_non_game,
        backup_dir: backup_dir.to_string_lossy().to_string(),
        imported_titles,
        library,
    })
}

/// Runs 1-click Auto-Fix on a game: unblocks NTFS `:Zone.Identifier`, runs PE32/PE32+ DLL
/// inspection & redistributable healing, and initializes an isolated `.duke-profile/` sandbox.
#[tauri::command]
async fn auto_fix_game(app: AppHandle, game_id: String) -> Result<AutoFixReport, String> {
    let app_for_fix = app.clone();
    let gid = game_id.clone();

    let (report, updated_library) = tokio::task::spawn_blocking(move || -> Result<(AutoFixReport, Vec<GameEntry>), String> {
        let mut library = load_library_from_disk(&app_for_fix)?;
        let Some(game_idx) = library.iter().position(|g| g.id == gid) else {
            return Err("Game not found in library.".to_string());
        };

        let exe_path = PathBuf::from(library[game_idx].exe_path.trim());
        if !exe_path.is_file() {
            return Err(format!("Game executable not found: {}", exe_path.display()));
        }
        let game_dir = if !library[game_idx].install_dir.trim().is_empty()
            && Path::new(library[game_idx].install_dir.trim()).is_dir()
        {
            PathBuf::from(library[game_idx].install_dir.trim())
        } else {
            resolve_game_root_dir(&exe_path)
        };

        let mut unblocked_files = unblock_ntfs_zone_identifier_raw(&game_dir);
        if !exe_path.starts_with(&game_dir) {
            unblocked_files += unblock_ntfs_zone_identifier_raw(&exe_path);
        }

        let preflight = pe_inspect::inspect_exe_with_dirs(&exe_path, None, &[])?;
        let missing_names: Vec<String> = preflight.missing_dlls.iter().map(|d| d.name.clone()).collect();
        let missing_dlls_found = missing_names.len();
        let mut healed_dlls = Vec::new();
        let mut unresolved_dlls = missing_names.clone();

        if !missing_names.is_empty() {
            let heal = pe_inspect::heal_missing_dlls_into_game_dir(
                &exe_path,
                Some(&missing_names),
                None,
                &[],
            )?;
            healed_dlls = heal.copied_dlls;
            unresolved_dlls = heal.skipped_dlls;

            if !unresolved_dlls.is_empty() {
                if let Some(repair_note) =
                    self_heal::try_silent_redist_repair(&game_dir, &unresolved_dlls, preflight.is_32bit)
                {
                    if let Ok(re_check) = pe_inspect::inspect_exe_with_dirs(&exe_path, None, &[]) {
                        let rem: Vec<String> = re_check
                            .missing_dlls
                            .iter()
                            .map(|d| d.name.clone())
                            .collect();
                        if let Ok(re_heal) =
                            pe_inspect::heal_missing_dlls_into_game_dir(&exe_path, Some(&rem), None, &[])
                        {
                            for c in re_heal.copied_dlls {
                                if !healed_dlls.contains(&c) {
                                    healed_dlls.push(c);
                                }
                            }
                            unresolved_dlls = re_heal.skipped_dlls;
                        } else {
                            unresolved_dlls = rem;
                        }
                    }
                    if !repair_note.is_empty() && !healed_dlls.iter().any(|x| x == &repair_note) {
                        healed_dlls.push(repair_note);
                    }
                }
            }
        }

        let mut enabled_laa_32bit = false;
        if preflight.is_32bit {
            let eff_size = library[game_idx].size_bytes.max(calculate_dir_size_sync(&game_dir));
            if self_heal::should_enable_32bit_laa(&exe_path, &game_dir, eff_size)
                && matches!(pe_inspect::set_pe_large_address_aware(&exe_path, true), Ok(true))
            {
                enabled_laa_32bit = true;
            }
        }

        let env_map = pe_inspect::prepare_game_sandbox_env(&game_dir)?;
        let sandbox_path = env_map
            .iter()
            .find(|(k, _)| k == "APPDATA")
            .and_then(|(_, p)| Path::new(p).parent())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| game_dir.join(".duke-profile").to_string_lossy().to_string());

        library[game_idx].sandbox_enabled = true;
        let title = library[game_idx].title.clone();
        save_library_to_disk(&app_for_fix, &library)?;

        let mut parts = Vec::new();
        if unblocked_files > 0 {
            parts.push(format!("Unblocked {} Windows-tagged files", unblocked_files));
        } else {
            parts.push("Checked file safety tags (all clear)".to_string());
        }
        if !healed_dlls.is_empty() {
            parts.push(format!(
                "repaired {} missing Windows runtime file(s) ({})",
                healed_dlls.len(),
                healed_dlls.join(", ")
            ));
        } else if unresolved_dlls.is_empty() {
            parts.push("verified all Windows runtime files are present".to_string());
        } else {
            parts.push(format!(
                "{} file(s) still missing ({})",
                unresolved_dlls.len(),
                unresolved_dlls.join(", ")
            ));
        }
        if enabled_laa_32bit {
            parts.push(
                "enabled 4 GB Large Address Aware flag for >1.5 GB 32-bit executable".to_string(),
            );
        }
        parts.push("enabled isolated game save folder (.duke-profile)".to_string());
        let summary = format!("{}.", parts.join(", "));

        let report = AutoFixReport {
            game_id: gid,
            title,
            unblocked_files,
            missing_dlls_found,
            healed_dlls,
            unresolved_dlls,
            sandbox_path,
            sandbox_enabled: true,
            summary,
        };
        Ok((report, library))
    })
    .await
    .map_err(|e| format!("Auto-Fix worker failed: {}", e))??;

    let _ = app.emit("library-updated", &updated_library);
    Ok(report)
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
async fn get_disk_free_space(
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
fn register_duke_protocol() -> Result<String, String> {
    win32_clipboard_protocol::register_duke_uri_protocol();
    Ok("Registered duke:// one-click browser download links with Windows.".to_string())
}

/// Runs the Duke Suite hardware subsystem microbenchmark suite inline for Advanced Engine settings.
#[tauri::command]
async fn run_subsystem_benchmark() -> Result<Value, String> {
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

fn parse_version_parts(raw: &str) -> Vec<u64> {
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

async fn download_url_bytes_via_doh(url: &str, dest: &Path) -> Result<(), String> {
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

struct DukeProfilePreserveGuard {
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
async fn set_game_update_config(
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
async fn check_game_updates(
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
async fn apply_game_update(
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
async fn toggle_overlay_window(app: AppHandle, visible: Option<bool>) -> Result<bool, String> {
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
async fn hide_overlay_window(app: AppHandle) -> Result<(), String> {
    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.hide();
    }
    trim_process_working_set();
    Ok(())
}

#[tauri::command]
async fn trigger_replay_clip_ipc() -> Result<Value, String> {
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

    Ok(serde_json::json!({
        "dispatched": true,
        "gameTitle": active_title,
        "message": "Instant Replay save signal transmitted over Duke SHM & IPC buses."
    }))
}

#[tauri::command]
async fn purge_suite_ram_ipc(app: AppHandle) -> Result<Value, String> {
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

fn spawn_overlay_trigger_watcher(app: AppHandle) {
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
            loop {
                let mut should_toggle_overlay = false;
                let mut should_toggle_guide = false;
                let mut should_toggle_radial = false;
                let mut should_vision = false;

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

                let pad = duke_core::win32_ffi::poll_xinput_state();
                let vision_chord_down = pad.connected && pad.guide && pad.y;
                let radial_chord_down = pad.connected && pad.guide && pad.lb;
                if vision_chord_down && !vision_chord_was_down {
                    should_vision = true;
                } else if radial_chord_down && !radial_chord_was_down {
                    should_toggle_radial = true;
                } else if pad.connected && pad.guide && !pad.y && !pad.lb && !guide_was_down {
                    should_toggle_guide = true;
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
                    couch_mode::handle_controller_guide_event(&app, "toggle_radial", "xbox", None);
                } else if should_toggle_guide {
                    let app_guide = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let main_visible = app_guide
                            .get_webview_window("main")
                            .map(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false))
                            .unwrap_or(false);
                        if main_visible {
                            couch_mode::handle_controller_guide_event(
                                &app_guide,
                                "toggle_couch",
                                "xbox",
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

                std::thread::sleep(Duration::from_millis(55));
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
async fn backup_game_saves(
    _app: AppHandle,
    game_id: Option<String>,
    title: Option<String>,
    install_dir: Option<String>,
    exe_path: Option<String>,
) -> Result<String, String> {
    crate::ludusavi::backup_game_saves_core(game_id, title, install_dir, exe_path).await
}

fn shutdown_flush_downloads(app_handle: &AppHandle) {
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

mod crash_guard {
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
async fn get_frame_pacing_and_ioring_status() -> SystemRuntimeDiagnostics {
    SystemRuntimeDiagnostics {
        ioring: duke_core::nvme_prefetch::probe_ioring_support(),
        frame_pacing: duke_core::win32_ffi::inspect_frame_pacing_status(),
        gpu_hardware: shader_swarm::detect_gpu_hardware_identity(),
        projfs: projfs_stream::probe_projfs_status(),
        quick_resume: quick_resume::build_summary_snapshot(),
        steam_cm: steam_cm::get_steam_cm_status().await,
        store_direct: store_direct::get_store_direct_status().await,
    }
}

#[tauri::command]
async fn toggle_half_ms_frame_pacing(
    enabled: bool,
) -> Result<duke_core::win32_ffi::FramePacingStatus, String> {
    if enabled {
        Ok(duke_core::win32_ffi::begin_frame_pacing_runtime())
    } else {
        Ok(duke_core::win32_ffi::end_frame_pacing_runtime())
    }
}

#[tauri::command]
async fn purge_stale_driver_shaders() -> Result<duke_core::win32_ffi::ShaderCachePurgeReport, String> {
    let app_root = get_app_data_file_standalone("shader_swarm")?
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir);
    let drv = duke_core::win32_ffi::detect_primary_gpu_driver_version();
    duke_core::win32_ffi::check_and_purge_stale_vendor_shader_caches(&app_root, drv.as_deref())
}

fn main() {
    crash_guard::install();

    if std::env::args().any(|a| a == "--mcp") {
        mcp::run_mcp_stdio_server();
        return;
    }

    if std::env::args().any(|a| a == "--bench") {
        if let Err(err) = bench::run_cli_benchmark() {
            eprintln!("duke-launcher --bench error: {err}");
            std::process::exit(1);
        }
        return;
    }

    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    couch_mode::init_cli_couch_flag(&raw_args);
    if !win32_clipboard_protocol::acquire_single_instance_or_forward(&raw_args) {
        return;
    }
    let initial_protocol_urls: Vec<String> = raw_args
        .iter()
        .filter_map(|arg| parse_duke_protocol_download_url(arg))
        .collect();

    let _ = get_active_hotkey_cell();
    let daemon_state = duke_core::tray_daemon::TrayDaemonState::new(
        duke_core::tray_daemon::TrayDaemonConfig {
            tray_id: "duke-launcher-tray".to_string(),
            window_label: "main".to_string(),
            tooltip: "Duke Launcher (Zero-RAM Tray Daemon)".to_string(),
            open_label: "Open Duke Launcher".to_string(),
            trim_label: "Trim RAM Now".to_string(),
            exit_label: "Exit Duke Launcher".to_string(),
            close_to_tray: Arc::new(AtomicBool::new(true)),
            minimize_to_tray: Arc::new(AtomicBool::new(true)),
        },
    );
    let _ = TRAY_DAEMON.set(daemon_state.clone());

    let daemon_for_setup = daemon_state.clone();
    let app = match tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .setup(move |app| {
            win32_clipboard_protocol::register_duke_uri_protocol();
            win32_clipboard_protocol::start_protocol_pipe_server(app.handle().clone());

            let exit_app = app.handle().clone();
            let on_exit_cb: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                if let Some(cell) = DUKE_SHM_BUS.get() {
                    if let Ok(guard) = cell.lock() {
                        if let Some(ref bus) = *guard {
                            bus.unregister_app();
                        }
                    }
                }
                shutdown_flush_downloads(&exit_app);
            });
            let _ = duke_core::tray_daemon::install_standard_tray(
                app.handle(),
                &daemon_for_setup,
                None,
                Some(on_exit_cb),
            );
            if let Some(tray) = app.tray_by_id("duke-launcher-tray") {
                if let Ok(dyn_img) = image::load_from_memory(include_bytes!("../icons/32x32.png")) {
                    let rgba = dyn_img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    let tray_img = tauri::image::Image::new_owned(rgba.into_raw(), w, h);
                    let _ = tray.set_icon(Some(tray_img));
                }
            }

            for initial_url in &initial_protocol_urls {
                enqueue_protocol_download_url(
                    app.handle().clone(),
                    initial_url.clone(),
                    false,
                    true,
                );
            }

            // Ensure covers/, sidebar_icons/, and backups/ directories exist (supports Portable Mode if portable.flag or data/ is present)
            if let Ok(covers_dir) = get_covers_dir(app.handle()) {
                let _ = app.asset_protocol_scope().allow_directory(&covers_dir, true);
            }
            let _ = sidebar_icons::get_sidebar_icons_dir(app.handle());
            if resolve_portable_data_dir().is_some() {
                sidebar_icons::allow_portable_sidebar_icons_scope(app.handle());
            }
            let _ = get_backups_dir(app.handle());

            // Run non-blocking startup self-healing (.bak JSON recovery, paused download reset, orphan tmp & dkcat verification).
            self_heal::spawn_startup_self_heal(app.handle().clone());

            // Non-blocking Driver Profile Guard: detects DXGI driver updates and re-applies UserGpuPreferences & Auto-SR profiles.
            latency_lab::spawn_startup_driver_profile_guard(app.handle().clone());

            // Non-blocking background sync of Nintendo & retro Libretro RDB catalog into `catalog.dkcat` if empty
            retro_catalog::spawn_background_retro_catalog_sync_if_needed(app.handle().clone());

            // Initialize cache-line-aligned lock-free Win32 shared memory event bus (`Local\duke-suite-shm-v1`).
            let _ = DUKE_SHM_BUS.get_or_init(|| {
                let bus = DukeShmBus::open_or_create().ok();
                if let Some(ref b) = bus {
                    b.register_app(ShmSourceApp::DukeLauncher);
                }
                Mutex::new(bus)
            });

            // Map `catalog.dkcat` via zero-copy mmap on startup (`0 ms` parse, `0 KB` heap).
            let _ = catalog_mmap::ensure_catalog_mapped_standalone();

            let mut initial_settings = load_settings_from_disk(app.handle());
            if couch_mode::is_cli_couch_booted() {
                initial_settings.couch_mode = true;
            }
            apply_runtime_settings(&initial_settings);

            if let Some(overlay) = app.get_webview_window("overlay") {
                let _ = overlay.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
            }

            if std::env::args().any(|a| a == "--minimized") {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.minimize();
                    let _ = win.hide();
                }
            }

            // Start Duke Swarm LAN/Tailscale discovery only if enabled by user settings.
            if initial_settings.swarm_enabled {
                swarm::start_swarm_background_services();
            }

            // Resolve missing retail prices for `turbo_downloader` games non-blockingly (`catalog.dkcat` -> DoH).
            spawn_startup_turbo_price_resolution(app.handle().clone());

            // Start XInput Guide button (`0x0400`) + global `Alt+G` hotkey listener for quick overlay HUD.
            spawn_overlay_trigger_watcher(app.handle().clone());

            // Start leader-elected Duke Suite IPC bus
            let bus_app = app.handle().clone();
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
                        duke_director::on_achievement_unlocked_pulse(
                            &bus_app,
                            &game_title,
                            &title_slug,
                        );
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

            // Trim working-set immediately after startup initialization (unless RAM trim is set to Off)
            if RAM_TRIM_MODE.load(Ordering::Relaxed) != 0 {
                trim_process_working_set();
            }
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut tick_count: u64 = 0;
                let mut interval = tokio::time::interval(Duration::from_secs(15));
                loop {
                    interval.tick().await;
                    tick_count = tick_count.saturating_add(1);
                    let _ = duke_ai_rt::unload_idle_workers_internal(false);
                    let ram_mode = RAM_TRIM_MODE.load(Ordering::Relaxed);
                    let any_downloading = {
                        let guard = get_queue_store().lock().unwrap_or_else(|p| p.into_inner());
                        guard.iter().any(|j| {
                            j.status == "DOWNLOADING"
                                || j.status == "RESOLVING"
                                || j.status == "INSTALLING"
                        })
                    };
                    let is_headless_or_minimized = app_handle
                        .get_webview_window("main")
                        .map(|w| w.is_minimized().unwrap_or(false))
                        .unwrap_or(true);

                    let should_trim = match ram_mode {
                        0 => false,                                       // Off
                        2 => !any_downloading || is_headless_or_minimized, // Aggressive: every 15s
                        _ => (tick_count % 4 == 0) && (!any_downloading || is_headless_or_minimized), // Standard: every 60s
                    };
                    if should_trim {
                        if ram_mode == 2 && is_headless_or_minimized {
                            duke_core::win32_ffi::enable_idle_eco_qos_mode();
                        }
                        trim_process_working_set();
                    }
                    if !(ram_mode == 2 || tick_count % 4 == 0) {
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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_version,
            apply_window_backdrop,
            enqueue_download,
            install_downloaded_job,
            clear_completed_downloads,
            set_speed_limit,
            pause_download,
            resume_download,
            remove_download_job,
            get_download_jobs,
            get_launcher_settings,
            update_launcher_settings,
            fetch_catalog_games,
            enrich_catalog_query,
            get_catalog_game_details,
            auto_fix_all_covers,
            find_and_apply_cover,
            scan_file_security,
            delete_flagged_file,
            get_directory_size,
            add_local_game,
            link_local_game_with_steam_id,
            set_game_launch_args,
            compress_game_folder,
            change_game_exe,
            launch_local_game,
            launch_game,
            get_game_steam_details,
            sync_library_steam_prices,
            get_local_library,
            sidebar_icons::get_sidebar_icons,
            remove_local_game,
            toggle_favorite_game,
            open_game_folder,
            trim_working_set,
            get_system_telemetry,
            backup_game_saves,
            sources::get_imported_sources,
            sources::search_imported_sources,
            sources::import_source_url,
            sources::import_source_file,
            sources::toggle_imported_source,
            sources::refresh_imported_source,
            sources::refresh_all_imported_sources,
            sources::remove_imported_source,
            sites::get_search_sites,
            sites::add_search_site,
            sites::remove_search_site,
            sites::bulk_import_search_sites,
            sites::reset_default_search_sites,
            open_directory_browser,
            ludusavi::sync_ludusavi_manifest,
            ludusavi::discover_game_save_paths,
            ludusavi::list_save_backups,
            ludusavi::restore_game_save,
            importers::import_existing_libraries,
            artwork::search_steamgriddb_artwork,
            artwork::apply_custom_artwork,
            artwork::create_desktop_shortcut_with_icon,
            artwork::resolve_catalog_cover,
            suspend_or_resume_game,
            get_active_game_sessions,
            poll_gamepad_state,
            read_clipboard_text,
            write_clipboard_text,
            take_pending_protocol_urls,
            set_game_launch_profile,
            snapshots::create_game_snapshot,
            snapshots::list_game_snapshots,
            snapshots::restore_game_snapshot,
            snapshots::delete_game_snapshot,
            snapshots::reclaim_snapshot_space,
            ingest_dropped_paths,
            scan_and_clean_desktop_shortcuts,
            auto_fix_game,
            get_disk_free_space,
            register_duke_protocol,
            run_subsystem_benchmark,
            catalog_mmap::get_catalog_mmap_stats,
            catalog_mmap::search_catalog_mmap,
            catalog_mmap::query_catalog_mmap,
            delta_patch::generate_game_dkmanifest,
            delta_patch::diff_game_against_manifest,
            delta_patch::create_delta_patch_file,
            delta_patch::apply_game_delta_patch,
            swarm::get_swarm_status,
            swarm::probe_swarm_peers,
            swarm::beam_game_from_peer,
            swarm::sync_game_saves_mesh,
            pe_inspect::preflight_check_exe,
            pe_inspect::heal_game_runtime_dlls,
            pe_inspect::get_game_compat_status,
            pe_inspect::toggle_game_laa_patch,
            pe_inspect::toggle_game_dxvk_wrapper,
            friends::get_unified_friends,
            friends::join_friend_session,
            importers::sync_storefront_playtime,
            achievements::get_game_achievements,
            achievements::set_game_achievements_path,
            ntfs_compress::get_game_storage_stats,
            ntfs_compress::compress_game_wof,
            ntfs_compress::decompress_game_wof,
            metadata_ext::get_game_extended_telemetry,
            set_game_update_config,
            check_game_updates,
            apply_game_update,
            toggle_overlay_window,
            hide_overlay_window,
            trigger_replay_clip_ipc,
            purge_suite_ram_ipc,
            retro_catalog::sync_retro_catalog,
            retro_catalog::get_retro_platforms,
            emulators::get_emulator_statuses,
            emulators::install_emulator_for_console,
            emulators::set_custom_emulator_path,
            emulators::check_game_emulator_ready,
            inspect_rom_path,
            add_retro_game_from_catalog,
            link_game_rom,
            scan_rom_folder,
            get_frame_pacing_and_ioring_status,
            toggle_half_ms_frame_pacing,
            purge_stale_driver_shaders,
            projfs_stream::get_projfs_runtime_status,
            projfs_stream::enable_projfs_windows_feature,
            projfs_stream::mount_projfs_virtual_game,
            projfs_stream::unmount_projfs_virtual_game,
            projfs_stream::deduplicate_library_cas,
            shader_swarm::get_shader_swarm_status,
            shader_swarm::sync_game_shaders_from_swarm,
            shader_swarm::archive_game_shader_bundle,
            quick_resume::get_quick_resume_slots,
            quick_resume::quick_switch_game_slot,
            quick_resume::suspend_to_quick_resume_slot,
            quick_resume::remove_quick_resume_slot,
            steam_cm::get_steam_cm_status,
            steam_cm::connect_steam_cm,
            steam_cm::disconnect_steam_cm,
            steam_cm::send_steam_chat_message,
            steam_cm::get_steam_chat_messages,
            steam_cm::get_steam_pending_invites,
            steam_cm::respond_steam_game_invite,
            steam_cm::list_steam_cloud_files,
            steam_cm::sync_steam_cloud_saves,
            store_direct::get_store_direct_status,
            store_direct::login_gog_direct,
            store_direct::login_epic_direct,
            store_direct::list_gog_owned_titles,
            store_direct::list_epic_owned_titles,
            store_direct::start_store_direct_install,
            mod_graph::scan_game_mod_graph,
            mod_graph::register_game_mod_pack,
            mod_graph::save_game_mod_loadout,
            mod_graph::activate_game_mod_loadout,
            get_suite_autonomy_status,
            duke_link::get_duke_link_status,
            duke_link::start_coop_host,
            duke_link::stop_coop_session,
            duke_link::invite_friend_to_coop,
            duke_link::join_coop_stream,
            duke_link::send_coop_input_p2,
            time_machine::get_game_time_machine,
            time_machine::create_time_machine_checkpoint,
            time_machine::create_time_machine_branch,
            time_machine::switch_time_machine_branch,
            time_machine::restore_time_machine_checkpoint,
            time_machine::delete_time_machine_checkpoint,
            direct_storage::probe_direct_storage_capability,
            direct_storage::scan_direct_storage_assets,
            direct_storage::repack_game_direct_storage_assets,
            direct_storage::verify_direct_storage_streams,
            vhdx_stage::get_vhdx_stage_status,
            vhdx_stage::containerize_game_to_vhdx,
            vhdx_stage::stage_game_hot_tier,
            vhdx_stage::unstage_or_detach_vhdx,
            duke_upscale::get_game_upscale_profile,
            duke_upscale::save_game_upscale_profile,
            duke_upscale::promote_game_window_borderless,
            latency_lab::get_latency_lab_status,
            latency_lab::get_game_latency_profile,
            latency_lab::apply_game_latency_profile,
            latency_lab::remove_game_latency_profile,
            latency_lab::run_driver_profile_guard,
            latency_lab::enforce_shader_cache_cap,
            etw_xray::probe_etw_xray_capability,
            etw_xray::get_game_xray_scorecards,
            etw_xray::clear_game_xray_scorecards,
            deep_shrink::scan_game_deep_shrink,
            deep_shrink::apply_game_deep_shrink,
            deep_shrink::deduplicate_library_blake3,
            duke_ai_rt::get_ai_runtime_status,
            duke_ai_rt::update_ai_runtime_config,
            duke_ai_rt::unload_ai_workers,
            duke_vision::trigger_duke_vision_capture,
            duke_vision::analyze_vision_text_query,
            duke_vision::get_latest_vision_state,
            duke_comms::get_duke_comms_state,
            duke_comms::toggle_duke_comms_listening,
            duke_comms::process_voice_phrase_command,
            duke_comms::upsert_voice_macro_rule,
            duke_doctor::diagnose_game_crash,
            duke_doctor::doctor_revert_game_snapshot,
            duke_director::get_duke_director_state,
            duke_director::toggle_duke_director,
            duke_director::trigger_director_highlight_now,
            duke_configurator::analyze_game_config_preset,
            duke_configurator::apply_game_config_preset,
            duke_configurator::rollback_game_config_preset,
            duke_acoustics::get_acoustics_state,
            duke_acoustics::set_wasapi_session_volume,
            duke_acoustics::save_game_eq_and_denoise,
            duke_acoustics::test_mic_rnnoise_latency,
            catalog_mmap::get_play_tonight_recommendations,
            couch_mode::get_couch_mode_status,
            couch_mode::set_couch_mode_state,
            couch_mode::trigger_couch_radial_action,
            dkpack::export_game_dkpack,
            dkpack::inspect_dkpack_file,
            dkpack::apply_dkpack_recipe,
            duke_capsule::export_game_capsule,
            duke_capsule::inspect_game_capsule,
            duke_pilot::plan_duke_pilot_command,
            duke_pilot::execute_duke_pilot_plan,
            duke_jail::get_game_jail_status,
            duke_jail::set_game_jail_policy,
            duke_jail::cleanup_game_jail_profile,
            privacy_vault::get_privacy_vault_status,
            privacy_vault::set_vault_api_key,
            privacy_vault::apply_anonymity_network_preset,
            fix_finder::analyze_game_crash_or_preflight,
            fix_finder::apply_one_click_game_fixes,
            fix_finder::fetch_pcgamingwiki_fixes,
            pe_inspect::check_sha256_threat_feed,
            unblock_motw_for_path,
            bios_vault::get_bios_vault_status,
            bios_vault::scan_and_install_bios_sources,
            retro_achievements::fetch_retro_achievements_progress,
            rom_patcher::group_multi_disc_library,
            rom_patcher::apply_rom_patch_and_create_profile,
            uninstaller::preview_clean_uninstall,
            uninstaller::execute_clean_uninstall,
            mod_dropper::install_mod_archive_or_workshop,
            duke_verified::get_duke_verified_status,
            duke_verified::record_duke_verified_feedback,
            duke_verified::export_duke_verified_matrix,
            duke_verified::import_duke_verified_matrix,
            deep_shrink::evaluate_disk_pressure_autopilot,
            deep_shrink::execute_disk_pressure_autopilot,
            duke_ghost::list_game_ghosts,
            duke_ghost::get_ghost_status,
            duke_ghost::start_ghost_recording,
            duke_ghost::stop_ghost_recording,
            duke_ghost::start_ghost_playback,
            duke_ghost::stop_ghost_playback,
            duke_ghost::import_ghost_file,
            duke_ghost::export_ghost_file,
            duke_ghost::delete_ghost_recording,
            duke_sandbox::get_sandbox_trainer_status,
            duke_sandbox::sandbox_scan_memory,
            duke_sandbox::sandbox_write_memory_value,
            duke_sandbox::sandbox_set_speedhack_scale
        ])
        .build(tauri::generate_context!())
    {
        Ok(app) => app,
        Err(err) => {
            crash_guard::record_fatal_error(
                "tauri::Builder::build",
                &format!("failed to build Duke Launcher Tauri application: {err}"),
            );
            std::process::exit(1);
        }
    };

    app.run(move |app_handle, event| {
        if let tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::CloseRequested { api, .. },
            ..
        } = &event
        {
            if label == "overlay" {
                api.prevent_close();
                if let Some(overlay_win) = app_handle.get_webview_window("overlay") {
                    let _ = overlay_win.hide();
                }
                return;
            }
        }
        let exit_cb = || shutdown_flush_downloads(app_handle);
        duke_core::tray_daemon::handle_run_event(
            app_handle,
            &event,
            &daemon_state,
            None,
            Some(&exit_cb),
        );
    });
}

#[cfg(test)]
mod tests {
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
