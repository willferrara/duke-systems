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
    achievements, app_system, archive_native, artwork, bench, bios_vault, catalog_mmap, couch_mode,
    deep_shrink, delta_patch, desktop_chord, direct_storage, discord_ipc, dkpack, duke_acoustics,
    duke_ai_rt,
    duke_capsule, duke_comms, duke_configurator, duke_director, duke_doctor, duke_ghost,
    duke_jail, duke_link, duke_pilot, duke_sandbox, duke_upscale, duke_verified, duke_vision,
    emulators, etw_xray, fix_finder, friends, fse_shell, importers, latency_lab, lib_store,
    ludusavi, mcp, metadata_ext, mod_dropper, mod_graph, net_engine, ntfs_compress, pe_inspect,
    platforms, playtime_sync, privacy_vault, projfs_stream, quick_resume, redist_cleaner,
    retro_achievements, retro_catalog, rom_inspect, rom_patcher, rom_shrink, save_watcher,
    self_heal, shader_swarm, sidebar_icons, sites, snapshots, sources, steam_cm, steam_profile,
    store_direct, stream_extract, stress_harness, swarm, time_machine, uninstaller, vhdx_stage,
    wasm_plugins,
};
use crate::cmd_emulators::*;
use crate::cmd_security::*;
use crate::cmd_system::*;
pub(crate) static LIBRARY_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
pub(crate) static CATALOG_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
pub(crate) static LIVE_CATALOG_UPSERTS: OnceLock<Mutex<HashMap<u32, CatalogGameEntry>>> = OnceLock::new();
pub(crate) static PRICE_SYNC_ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) static CATALOG_SYNC_ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) const CATALOG_TARGET_COUNT: usize = 30_000;
pub(crate) const CATALOG_HARVEST_PAGES: u32 = 30;
pub(crate) const CATALOG_SEMAPHORE_CONCURRENCY: usize = 4;

// 512 KB per-worker BufWriter capacity to cut ~25 MB of idle heap allocations during 8-stream downloads
pub(crate) const MAX_COVER_IMAGE_BYTES: usize = 256 * 1024;
pub(crate) const CATALOG_TTL_SECS: u64 = 24 * 60 * 60;
pub(crate) const HTTP_METADATA_TIMEOUT_SECS: u64 = 10;
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
pub(crate) struct CatalogSyncProgressEvent {
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

pub(crate) const IGNORED_GAME_SUBDIRS: &[&str] = &[
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

pub(crate) const PENALIZED_HELPER_EXES: &[&str] = &[
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

#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
extern "system" {
    fn GetCompressedFileSizeW(lpFileName: *const u16, lpFileSizeHigh: *mut u32) -> u32;
}

/// Queries Windows NTFS physical allocated file size (`GetCompressedFileSizeW`) so NTFS LZX/XPRESS8K
/// compression savings from `compact.exe` are accurately reflected in disk usage telemetry.
pub(crate) fn get_physical_file_size_on_disk(path: &Path) -> u64 {
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
pub(crate) fn resolve_portable_data_dir() -> Option<PathBuf> {
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

pub(crate) fn get_backups_dir(app: &AppHandle) -> Result<PathBuf, String> {
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

pub(crate) fn get_app_data_file(app: &AppHandle, filename: &str) -> Result<PathBuf, String> {
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
pub(crate) fn persist_cover_to_disk_cache(app: &AppHandle, game_id: &str, cover_str: &str) -> String {
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

pub(crate) fn hydrate_cover_from_disk_cache(app: &AppHandle, game_id: &str, stored_cover: &str) -> String {
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

pub(crate) fn get_library_io_lock() -> &'static Mutex<()> {
    LIBRARY_IO_LOCK.get_or_init(|| Mutex::new(()))
}

pub(crate) fn get_catalog_io_lock() -> &'static Mutex<()> {
    CATALOG_IO_LOCK.get_or_init(|| Mutex::new(()))
}

pub(crate) fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    self_heal::atomic_write_with_backup(path, bytes)
}

pub(crate) fn atomic_write_string(path: &Path, contents: &str) -> Result<(), String> {
    atomic_write_bytes(path, contents.as_bytes())
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

pub(crate) fn persist_cover_to_disk_cache_standalone(game_id: &str, cover_str: &str) -> String {
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

pub(crate) fn hydrate_cover_from_disk_cache_standalone(game_id: &str, stored_cover: &str) -> String {
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
    let data_dir = resolve_data_root_dir_standalone()?;
    let mut entries = lib_store::load_library_dklib(&data_dir)?;
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
    let data_dir = resolve_data_root_dir_standalone()?;
    let mut disk_entries: Vec<GameEntry> = entries.to_vec();
    for entry in &mut disk_entries {
        if entry.cover_url.starts_with("data:image/") {
            entry.cover_url = persist_cover_to_disk_cache_standalone(&entry.id, &entry.cover_url);
        }
    }
    lib_store::save_library_dklib_full(&data_dir, &disk_entries)
}

pub(crate) fn load_catalog_cache_from_disk(app: &AppHandle) -> CatalogCacheFile {
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

pub(crate) fn save_catalog_cache_to_disk(app: &AppHandle, cache: &CatalogCacheFile) -> Result<(), String> {
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
    let data_dir = resolve_data_root_dir(app)?;
    let mut entries = lib_store::load_library_dklib(&data_dir)?;
    for entry in &mut entries {
        normalize_game_entry(entry);
    }
    Ok(entries)
}

pub(crate) fn load_library_from_disk(app: &AppHandle) -> Result<Vec<GameEntry>, String> {
    let _guard = get_library_io_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let data_dir = resolve_data_root_dir(app)?;
    let mut entries = lib_store::load_library_dklib(&data_dir)?;

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

pub(crate) fn save_library_to_disk(app: &AppHandle, entries: &[GameEntry]) -> Result<(), String> {
    let _guard = get_library_io_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let data_dir = resolve_data_root_dir(app)?;
    let mut disk_entries: Vec<GameEntry> = entries.to_vec();
    for entry in &mut disk_entries {
        if entry.cover_url.starts_with("data:image/") {
            entry.cover_url = persist_cover_to_disk_cache(app, &entry.id, &entry.cover_url);
        }
    }

    lib_store::save_library_dklib_full(&data_dir, &disk_entries)
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

pub(crate) async fn fetch_steam_app_cover_verified(app_id: u64) -> (Option<String>, Option<String>) {
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

#[allow(dead_code)]
pub(crate) fn extract_exe_icon_data_uri(exe_path: &str) -> Option<String> {
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

pub(crate) fn detect_steam_appid_in_folder(install_dir: &Path) -> Option<u64> {
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

pub(crate) async fn resolve_steam_appid_by_title(title: &str) -> Option<(u32, Option<String>)> {
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

pub(crate) fn extract_steam_price_from_appdetails_data(data: &Value) -> (u32, String) {
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

pub(crate) async fn fetch_steam_app_retail_price(appid: u32) -> (u32, String) {
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

pub(crate) async fn smart_find_game_cover(
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
            title: artwork::clean_cover_query(&lookup_title),
            exe_path: (!exe_path.trim().is_empty()).then(|| exe_path.to_string()),
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

    (String::new(), detected_app_id, None)
}

#[tauri::command]
pub(crate) async fn auto_fix_all_covers(
    app: AppHandle,
    force_all: Option<bool>,
) -> Result<Vec<GameEntry>, String> {
    let force_refresh_all = force_all.unwrap_or(false);
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
            if crate::is_shutting_down() {
                return None;
            }
            let has_valid_local = artwork::has_local_cover_file(&entry_c, &covers_c);
            if !force_refresh_all && has_valid_local {
                let src = sidebar_icons::SidebarIconSource::from(&entry_c);
                let _ = sidebar_icons::resolve_sidebar_icon_in_dirs(&src, &icons_c, &covers_c);
                return None;
            }

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
                .or_else(|| entry_c.steam_app_id.and_then(|v| u32::try_from(v).ok()))
                .or_else(|| {
                    let p = Path::new(&entry_c.install_dir);
                    p.exists()
                        .then(|| detect_steam_appid_in_folder(p))
                        .flatten()
                        .and_then(|v| u32::try_from(v).ok())
                });

            let req = artwork::CoverResolveRequest {
                cache_id: entry_c.id.clone(),
                title: artwork::clean_cover_query(&entry_c.title),
                exe_path: (!entry_c.exe_path.trim().is_empty()).then(|| entry_c.exe_path.clone()),
                platform_id: eff_pid,
                platform_hint: plat_hint,
                steam_app_id: steam_aid,
                existing_cover_url: (!entry_c.cover_url.trim().is_empty())
                    .then(|| entry_c.cover_url.trim().to_string()),
                steamgriddb_api_key: key_c,
                sidebar_icon_id: Some(entry_c.id.clone()),
            };
            let outcome = artwork::resolve_and_cache_cover_in_dirs_with_options(
                &req,
                &covers_c,
                &icons_c,
                true,
            )
            .await;
            if let Some(ref out) = outcome {
                return Some((entry_c.id, Some(out.cover_path.to_string_lossy().to_string())));
            }
            // Even when no remote cover image was found, still extract/generate 32x32 sidebar icon from .exe
            let src = sidebar_icons::SidebarIconSource::from(&entry_c);
            let _ = sidebar_icons::resolve_sidebar_icon_in_dirs(&src, &icons_c, &covers_c);
            // Clear corrupt/tiny/data-URI cover paths so the clean procedural fallback renders instead of a 32x32 icon.
            let trimmed = entry_c.cover_url.trim();
            if !has_valid_local
                && !trimmed.is_empty()
                && !trimmed.starts_with("http://")
                && !trimmed.starts_with("https://")
            {
                return Some((entry_c.id, None));
            }
            None
        });
    }

    let data_root_opt = crate::resolve_data_root_dir(&app).ok();
    let mut updated_map: HashMap<String, Option<String>> = HashMap::new();
    while let Some(res) = join_set.join_next().await {
        if let Ok(Some((gid, cover_opt))) = res {
            if let Some(ref cover_path) = cover_opt {
                if let Some(ref data_root) = data_root_opt {
                    let existing = lib.iter().find(|g| g.id == gid);
                    let _ = crate::lib_store::append_cover_wal(
                        data_root,
                        &gid,
                        cover_path,
                        existing.and_then(|g| g.hero_url.as_deref()),
                        existing.and_then(|g| g.icon_path.as_deref()),
                    );
                }
                emit_if_alive(
                    &app,
                    "cover-updated",
                    serde_json::json!({
                        "id": &gid,
                        "coverPath": cover_path,
                        "gameId": &gid,
                        "coverUrl": cover_path,
                    }),
                );
            }
            updated_map.insert(gid, cover_opt);
        }
    }

    let mut changed = false;
    for entry in &mut lib {
        if let Some(new_cover_opt) = updated_map.get(&entry.id) {
            let next_cover = new_cover_opt.as_deref().unwrap_or("");
            if entry.cover_url != next_cover {
                entry.cover_url = next_cover.to_string();
                changed = true;
            }
        }
    }

    if changed {
        let app_save = app.clone();
        let lib_save = lib.clone();
        run_blocking_timeout(15, "auto_fix_all_covers_save", move || {
            save_library_to_disk(&app_save, &lib_save)
        })
        .await?;
    }
    emit_if_alive(&app, "library-updated", &lib);
    emit_if_alive(&app, "sidebar-icons-updated", ());
    trim_process_working_set();
    Ok(lib)
}

#[tauri::command]
pub(crate) async fn find_and_apply_cover(
    app: AppHandle,
    id: String,
    custom_query: Option<String>,
) -> Result<Vec<GameEntry>, String> {
    let has_custom_query = custom_query
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());

    let app_init = app.clone();
    let id_init = id.clone();
    let query_clone = custom_query.clone();
    let (search_target, mut library) = run_blocking_timeout(20, "find_and_apply_cover_load", move || {
        let library = load_library_from_disk(&app_init)?;
        if has_custom_query {
            if let Ok(covers_dir) = get_covers_dir(&app_init) {
                artwork::clear_cached_cover_for_id(&covers_dir, &id_init);
            }
        }
        let target_info = library.iter().find(|g| g.id == id_init).map(|entry| {
            let search_title = query_clone
                .filter(|q| !q.trim().is_empty())
                .unwrap_or_else(|| entry.title.clone());
            (
                entry.id.clone(),
                search_title,
                entry.exe_path.clone(),
                entry.install_dir.clone(),
            )
        });
        Ok((target_info, library))
    })
    .await?;

    let Some((entry_id, search_title, exe_path, install_dir)) = search_target else {
        return Ok(library);
    };

    let (cover, app_id, matched_name) =
        smart_find_game_cover(&app, &entry_id, &search_title, &exe_path, &install_dir).await;

    if cover.is_empty() {
        let _ = run_blocking_timeout(5, "trim_cover_miss", || {
            trim_process_working_set();
            Ok(())
        })
        .await;
        return Err(format!(
            "No cover found for '{}'. Try typing a different title.",
            search_title
        ));
    }

    let app_save = app.clone();
    let updated_lib = run_blocking_timeout(20, "find_and_apply_cover_save", move || {
        let disk_cover_for_emit = cover.clone();
        if let Some(entry) = library.iter_mut().find(|g| g.id == entry_id) {
            entry.cover_url = cover;
            if let Some(sid) = app_id {
                entry.steam_app_id = Some(sid);
                entry.app_id = u32::try_from(sid).ok();
            }
            if let Some(official) = matched_name {
                entry.title = official;
            }
            let mut disk_entry = entry.clone();
            if disk_entry.cover_url.starts_with("data:image/") {
                disk_entry.cover_url =
                    persist_cover_to_disk_cache(&app_save, &disk_entry.id, &disk_entry.cover_url);
            }
            let wal_ok = resolve_data_root_dir(&app_save)
                .and_then(|d| lib_store::append_upsert_wal(&d, &disk_entry))
                .is_ok();
            if !wal_ok {
                save_library_to_disk(&app_save, &library)?;
            }
            sidebar_icons::invalidate_sidebar_icon(&app_save, &entry_id);
        }
        emit_if_alive(
            &app_save,
            "cover-updated",
            serde_json::json!({
                "id": &entry_id,
                "coverPath": &disk_cover_for_emit,
                "gameId": &entry_id,
                "coverUrl": &disk_cover_for_emit,
            }),
        );
        emit_if_alive(&app_save, "library-updated", &library);
        emit_if_alive(&app_save, "sidebar-icons-updated", ());
        trim_process_working_set();
        Ok(library)
    })
    .await?;

    Ok(updated_lib)
}

#[tauri::command]
pub(crate) async fn get_directory_size(path: String) -> Result<u64, String> {
    run_blocking_timeout(60, "get_directory_size", move || {
        let target = PathBuf::from(&path);
        if !target.exists() {
            return Err(format!("Target path does not exist: {}", path));
        }
        let resolved_dir = resolve_game_root_dir(&target);
        Ok(calculate_dir_size_sync(&resolved_dir))
    })
    .await
}

/// Resolves retail price first via `0 ms` memory-mapped `catalog.dkcat`, falling back to Steam store search/details over `net-doh`.
pub(crate) async fn resolve_game_retail_price_fast(
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

#[cfg_attr(not(test), allow(dead_code))]
pub fn compute_money_saved_cents(library: &[GameEntry]) -> u64 {
    library
        .iter()
        .filter(|g| g.acquisition_source == "turbo_downloader")
        .map(|g| u64::from(g.steam_price_cents.unwrap_or(0)))
        .sum()
}

pub(crate) fn spawn_startup_turbo_price_resolution(app: AppHandle) {
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
            emit_if_alive(&app, "library-updated", &updated_lib);
        }
    });
}

#[tauri::command]
pub(crate) async fn add_local_game(
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

pub(crate) async fn add_local_game_with_origin(
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
pub(crate) fn parse_cli_launch_args(raw: &str) -> Vec<String> {
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
pub(crate) async fn link_local_game_with_steam_id(
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
pub(crate) async fn set_game_launch_args(
    app: AppHandle,
    game_id: String,
    launch_args: Option<String>,
) -> Result<Vec<GameEntry>, String> {
    let cleaned_args = launch_args
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let app_clone = app.clone();
    run_blocking_timeout(15, "set_game_launch_args", move || {
        let mut library = load_library_from_disk(&app_clone)?;
        let Some(entry) = library
            .iter_mut()
            .find(|g| g.id == game_id || g.exe_path.eq_ignore_ascii_case(&game_id))
        else {
            return Err("Game entry not found in library.".to_string());
        };
        entry.launch_args = cleaned_args;
        let wal_ok = resolve_data_root_dir(&app_clone)
            .and_then(|d| {
                lib_store::append_launch_args_wal(
                    &d,
                    &entry.id,
                    entry.launch_args.as_deref().unwrap_or(""),
                )
            })
            .is_ok();
        if !wal_ok {
            save_library_to_disk(&app_clone, &library)?;
        }
        emit_if_alive(&app_clone, "library-updated", &library);
        Ok(library)
    })
    .await
}

#[tauri::command]
pub(crate) async fn set_game_launch_profile(
    app: AppHandle,
    game_id: String,
    sandbox_enabled: Option<bool>,
    nvme_prefetch_override: Option<String>,
) -> Result<Vec<GameEntry>, String> {
    let app_clone = app.clone();
    run_blocking_timeout(15, "set_game_launch_profile", move || {
        let mut library = load_library_from_disk(&app_clone)?;
        let Some(entry) = library
            .iter_mut()
            .find(|g| g.id == game_id || g.exe_path.eq_ignore_ascii_case(&game_id))
        else {
            return Err("Game entry not found in library.".to_string());
        };
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
        let wal_ok = resolve_data_root_dir(&app_clone)
            .and_then(|d| {
                lib_store::append_launch_profile_wal(
                    &d,
                    &entry.id,
                    entry.sandbox_enabled,
                    entry.nvme_prefetch_enabled,
                )
            })
            .is_ok();
        if !wal_ok {
            save_library_to_disk(&app_clone, &library)?;
        }
        emit_if_alive(&app_clone, "library-updated", &library);
        Ok(library)
    })
    .await
}

/// 1-Click NTFS LZX / XPRESS8K Game Folder Compression (`compact.exe /c /s:"<game_folder>" /a /i /exe:xpress8k`).
/// Compresses the game installation directory transparently on Windows 11 with zero runtime FPS penalty,
/// recalculates physical on-disk size via `GetCompressedFileSizeW`, and updates `size_bytes` in `library.json`.
pub(crate) async fn compress_game_folder_core(app: AppHandle, game_id: String) -> Result<Value, String> {
    compress_game_folder(app, game_id).await
}

#[tauri::command]
pub(crate) async fn compress_game_folder(app: AppHandle, game_id: String) -> Result<Value, String> {
    let app_clone = app.clone();
    run_blocking_timeout(300, "compress_game_folder", move || {
        let current_library = load_library_from_disk(&app_clone)?;
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

        let gid_clone = target_entry.id.clone();
        let initial_recorded_size = target_entry.size_bytes;
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
        emit_if_alive(&app_clone, "library-updated", &library);
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
}

/// Opens a native file picker (or accepts `new_exe_path`) so users can switch which `.exe` a library entry launches
/// (e.g., DX11 vs DX12, Vulkan, or Mod Launcher).
#[tauri::command]
pub(crate) async fn change_game_exe(
    app: AppHandle,
    game_id: String,
    new_exe_path: Option<String>,
) -> Result<Vec<GameEntry>, String> {
    let app_init = app.clone();
    let gid_init = game_id.clone();
    let (current_library, target_entry) = run_blocking_timeout(15, "change_game_exe_load", move || {
        let current_library = load_library_from_disk(&app_init)?;
        let target_entry = current_library
            .iter()
            .find(|g| g.id == gid_init || g.exe_path.eq_ignore_ascii_case(&gid_init))
            .cloned()
            .ok_or_else(|| "Game entry not found in library.".to_string())?;
        Ok((current_library, target_entry))
    })
    .await?;

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

    let app_save = app.clone();
    run_blocking_timeout(60, "change_game_exe_save", move || {
        let exe_file = PathBuf::from(chosen_path_str.trim().trim_matches('"'));
        if !exe_file.exists() || !exe_file.is_file() {
            return Err(format!(
                "Selected executable does not exist: {}",
                exe_file.display()
            ));
        }

        let new_install_dir = resolve_game_root_dir(&exe_file);
        let size_bytes = calculate_dir_size_sync(&new_install_dir);

        let mut library = load_library_from_disk(&app_save)?;
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
            let mut disk_entry = entry.clone();
            if disk_entry.cover_url.starts_with("data:image/") {
                disk_entry.cover_url =
                    persist_cover_to_disk_cache(&app_save, &disk_entry.id, &disk_entry.cover_url);
            }
            if resolve_data_root_dir(&app_save)
                .and_then(|d| lib_store::append_upsert_wal(&d, &disk_entry))
                .is_err()
            {
                save_library_to_disk(&app_save, &library)?;
            }
        } else {
            save_library_to_disk(&app_save, &library)?;
        }

        if let Some(gid) = updated_id {
            sidebar_icons::invalidate_sidebar_icon(&app_save, &gid);
        }
        emit_if_alive(&app_save, "library-updated", &library);
        Ok(library)
    })
    .await
}

pub(crate) fn build_steam_hero_candidates(appid: u32, _header_image: Option<&str>) -> Vec<String> {
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

pub(crate) fn decode_basic_html_entities(input: &str) -> String {
    input
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
}

pub(crate) fn strip_steam_clan_tokens(input: &str) -> String {
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

pub(crate) fn strip_bbcode_media_blocks(input: &str) -> String {
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

pub(crate) fn convert_html_to_bbcode_text(input: &str) -> String {
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

pub(crate) fn clean_steam_clan_tokens(raw: &str) -> String {
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

pub(crate) fn strip_bbcode_and_html(raw: &str) -> String {
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
pub(crate) async fn get_game_steam_details(
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
                        emit_if_alive(&app_clone, "library-updated", &lib);
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
pub(crate) async fn sync_library_steam_prices(app: AppHandle) -> Result<Vec<GameEntry>, String> {
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
            emit_if_alive(&app_for_save, "library-updated", &lib);
        }
        Ok(lib)
    })
    .await
    .map_err(|e| format!("Failed to persist Steam prices: {}", e));

    PRICE_SYNC_ACTIVE.store(false, Ordering::SeqCst);
    updated_lib?
}

pub(crate) fn json_val_to_u64(v: Option<&Value>) -> u64 {
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

pub(crate) fn json_val_to_u32(v: Option<&Value>) -> u32 {
    u32::try_from(json_val_to_u64(v).min(u32::MAX as u64)).unwrap_or(u32::MAX)
}

pub(crate) fn parse_steamspy_owners_band(raw: &str) -> (u64, u64) {
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

pub(crate) fn format_compact_owners_band(min: u64, max: u64, raw_label: &str) -> String {
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

pub(crate) fn push_unique_label(list: &mut Vec<String>, candidate: &str) {
    let clean = candidate.trim();
    if clean.is_empty() {
        return;
    }
    if !list.iter().any(|existing| existing.eq_ignore_ascii_case(clean)) {
        list.push(clean.to_string());
    }
}

pub(crate) fn apply_tag_and_category_flags(entry: &mut CatalogGameEntry) {
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

pub(crate) fn compute_catalog_value_score(entry: &CatalogGameEntry) -> f64 {
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

pub(crate) fn finalize_catalog_entry(entry: &mut CatalogGameEntry, now: u64) {
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

pub(crate) fn merge_steamspy_map(
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

pub(crate) fn merge_featuredcategories(
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

pub(crate) fn apply_appdetails_and_players_to_entry(
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

pub(crate) async fn enrich_catalog_entry_from_steam(
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

pub(crate) const CATALOG_GENRE_QUERIES: [(&str, &str, bool); 21] = [
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

pub(crate) struct CatalogSyncActiveGuard;

impl Drop for CatalogSyncActiveGuard {
    fn drop(&mut self) {
        if let Some(cell) = LIVE_CATALOG_UPSERTS.get() {
            cell.lock().unwrap_or_else(|e| e.into_inner()).clear();
        }
        CATALOG_SYNC_ACTIVE.store(false, Ordering::SeqCst);
    }
}

pub(crate) fn record_live_catalog_upsert(entry: &CatalogGameEntry) {
    if CATALOG_SYNC_ACTIVE.load(Ordering::Acquire) {
        let cell = LIVE_CATALOG_UPSERTS.get_or_init(|| Mutex::new(HashMap::new()));
        cell.lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(entry.app_id, entry.clone());
    }
}

pub(crate) fn drain_live_upserts_into(entries_by_id: &mut HashMap<u32, CatalogGameEntry>) {
    if let Some(cell) = LIVE_CATALOG_UPSERTS.get() {
        let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
        for (id, entry) in guard.drain() {
            entries_by_id.insert(id, entry);
        }
    }
}

pub(crate) async fn fetch_doh_json_with_retry(
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
pub(crate) enum SteamSpyFetchTask {
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

pub(crate) async fn execute_steamspy_batch(
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

pub(crate) async fn flush_catalog_snapshot_async(
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

pub(crate) fn emit_catalog_sync_progress(app: &AppHandle, indexed_count: usize, done: bool) {
    let payload = CatalogSyncProgressEvent {
        indexed_count,
        target_count: CATALOG_TARGET_COUNT.max(indexed_count),
        done,
    };
    emit_if_alive(app, "catalog-sync-progress", &payload);
}

pub(crate) struct BackgroundHarvestJob {
    app: AppHandle,
    semaphore: Arc<Semaphore>,
    entries_by_id: HashMap<u32, CatalogGameEntry>,
    fetched_pages: Vec<u32>,
    missing_pages: Vec<u32>,
    include_genres: bool,
    fetched_at: u64,
    now: u64,
}

pub(crate) async fn run_catalog_background_harvest(job: BackgroundHarvestJob) {
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
pub(crate) async fn fetch_catalog_games(
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
            from_cache: total_indexed > 0,
            stale_warning: None,
            live_search_matched: 0,
        });
    }

    // Instant view-switch guarantee (<100 ms): when !force_refresh, ALWAYS return immediately
    // (0 ms for mapped entries, or empty + live progress event on cold boot) and sync in background.
    if !force {
        if (total_indexed == 0 || !cache_is_fresh || !all_pages_present)
            && CATALOG_SYNC_ACTIVE
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            let bg_pages: Vec<u32> = if cache_is_fresh && total_indexed > 0 {
                missing_pages
            } else {
                fetched_pages.clear();
                (0..CATALOG_HARVEST_PAGES).collect()
            };
            let bg_fetched_at = if cache_is_fresh { fetched_at } else { now };
            let app_load = app.clone();
            let app_emit = app.clone();
            emit_catalog_sync_progress(&app, total_indexed, false);
            tokio::spawn(async move {
                let existing = tokio::task::spawn_blocking(move || load_catalog_cache_from_disk(&app_load))
                    .await
                    .unwrap_or_default();
                let mut entries_by_id =
                    HashMap::with_capacity(existing.entries.len().max(CATALOG_TARGET_COUNT));
                for mut item in existing.entries {
                    finalize_catalog_entry(&mut item, now);
                    if item.app_id > 0 && !item.name.is_empty() {
                        entries_by_id.insert(item.app_id, item);
                    }
                }
                let semaphore = Arc::new(Semaphore::new(CATALOG_SEMAPHORE_CONCURRENCY));
                emit_catalog_sync_progress(&app_emit, entries_by_id.len(), false);
                run_catalog_background_harvest(BackgroundHarvestJob {
                    app: app_emit,
                    semaphore,
                    entries_by_id,
                    fetched_pages,
                    missing_pages: bg_pages,
                    include_genres: true,
                    fetched_at: bg_fetched_at,
                    now,
                })
                .await;
            });
        }
        return Ok(CatalogResponse {
            games: Vec::new(),
            total_cached: total_indexed,
            fetched_at,
            from_cache: total_indexed > 0,
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
pub(crate) async fn enrich_catalog_query(
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
pub(crate) async fn get_catalog_game_details(
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
pub(crate) async fn get_local_library(app: AppHandle) -> Result<Vec<GameEntry>, String> {
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
pub(crate) async fn remove_local_game(app: AppHandle, id: String) -> Result<Vec<GameEntry>, String> {
    run_blocking_timeout(15, "remove_local_game", move || {
        let mut library = load_library_from_disk(&app)?;
        let removed_ids: Vec<String> = library
            .iter()
            .filter(|g| !(g.id != id && !g.exe_path.eq_ignore_ascii_case(&id)))
            .map(|g| g.id.clone())
            .collect();
        library.retain(|g| g.id != id && !g.exe_path.eq_ignore_ascii_case(&id));
        let wal_ok = if removed_ids.is_empty() {
            true
        } else {
            resolve_data_root_dir(&app)
                .and_then(|d| {
                    for gid in &removed_ids {
                        lib_store::append_delete_wal(&d, gid)?;
                    }
                    Ok(())
                })
                .is_ok()
        };
        if !wal_ok {
            save_library_to_disk(&app, &library)?;
        }
        sidebar_icons::invalidate_sidebar_icon(&app, &id);
        for gid in removed_ids {
            sidebar_icons::invalidate_sidebar_icon(&app, &gid);
        }
        Ok(library)
    })
    .await
}

#[tauri::command]
pub(crate) async fn toggle_favorite_game(app: AppHandle, id: String) -> Result<Vec<GameEntry>, String> {
    run_blocking_timeout(15, "toggle_favorite_game", move || {
        let mut library = load_library_from_disk(&app)?;
        let mut updated_pair: Option<(String, bool)> = None;
        if let Some(entry) = library
            .iter_mut()
            .find(|g| g.id == id || g.exe_path.eq_ignore_ascii_case(&id))
        {
            entry.is_favorite = !entry.is_favorite;
            updated_pair = Some((entry.id.clone(), entry.is_favorite));
        }
        if let Some((ref gid, fav)) = updated_pair {
            let wal_ok = resolve_data_root_dir(&app)
                .and_then(|d| lib_store::append_favorite_wal(&d, gid, fav))
                .is_ok();
            if !wal_ok {
                save_library_to_disk(&app, &library)?;
            }
        }
        Ok(library)
    })
    .await
}

#[tauri::command]
pub(crate) async fn open_game_folder(path: String) -> Result<(), String> {
    run_blocking_timeout(10, "open_game_folder", move || {
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
    })
    .await
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

pub(crate) fn expand_windows_env_placeholders(input: &str) -> String {
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

pub(crate) fn wide_buf_to_trimmed_string(buf: &[u16]) -> String {
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

pub(crate) fn read_steam_appid_from_folder(folder: &Path) -> Option<u32> {
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

pub(crate) fn match_catalog_appid_for_title(title: &str) -> Option<u32> {
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

pub(crate) async fn apply_shortcut_launch_args_if_empty(
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

pub(crate) async fn ingest_dropped_paths_core(
    app: AppHandle,
    paths: Vec<String>,
) -> Result<IngestPathsReport, String> {
    ingest_dropped_paths(app, paths).await
}

/// Canonicalizes a filesystem path while stripping Windows `\\?\` verbatim prefixes so duplicate
/// drops, symlinks, casing variations, and relative segments resolve to one stable key.
pub(crate) fn canonicalize_drop_path(path: &Path) -> PathBuf {
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
pub(crate) async fn ingest_dropped_paths(
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
                    emit_if_alive(&app_for_roms, "library-updated", &lib);
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
                    emit_if_alive(&app, "dkpack-applied", &report);
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
                emit_if_alive(&app_for_rom, "library-updated", &lib);
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
    emit_if_alive(&app, "library-updated", &library);
    if !added_titles.is_empty() {
        spawn_background_missing_covers_resolver(app.clone());
    }

    Ok(IngestPathsReport {
        added_count: added_titles.len(),
        skipped_count: skipped_reasons.len(),
        added_titles,
        skipped_reasons,
        library,
    })
}

#[allow(dead_code)]
pub(crate) fn is_likely_non_game_shortcut(lnk_path: &Path, target_exe: &Path) -> bool {
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
#[allow(dead_code)]
pub(crate) struct DesktopShortcutCandidate {
    pub(crate) lnk_path: PathBuf,
    pub(crate) title: String,
    pub(crate) resolved: ResolvedShortcut,
}

#[allow(dead_code)]
pub(crate) fn move_file_with_unique_name(src: &Path, dest_dir: &Path) -> Result<PathBuf, String> {
    if let Some(reason) = crate::uninstaller::protected_path_refusal_reason(src) {
        return Err(format!("Refused moving protected file: {reason}"));
    }
    if let Some(reason) = crate::uninstaller::protected_path_refusal_reason(dest_dir) {
        return Err(format!("Refused writing into protected folder: {reason}"));
    }
    fs::create_dir_all(dest_dir)
        .map_err(|e| format!("Failed creating backup folder: {}", e))?;
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
            .map_err(|e| format!("Failed copying file '{}': {}", src.display(), e))?;
        if !crate::uninstaller::is_protected_user_or_system_path(src) {
            let _ = fs::remove_file(src);
        }
    }
    Ok(candidate)
}

#[allow(dead_code)]
pub(crate) fn collect_desktop_game_shortcuts_sync(
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

#[allow(dead_code)]
pub(crate) fn resolve_user_and_public_desktop_dirs() -> Result<(PathBuf, Vec<PathBuf>), String> {
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

/// Neutered for filesystem safety: Duke Launcher never moves, modifies, or deletes `.lnk`
/// shortcuts, folders, or files on the user's Desktop or Public Desktop.
#[tauri::command]
pub(crate) async fn scan_and_clean_desktop_shortcuts(app: AppHandle) -> Result<DesktopCleanupReport, String> {
    let library = tokio::task::spawn_blocking({
        let app_clone = app.clone();
        move || load_library_from_disk(&app_clone)
    })
    .await
    .map_err(|e| format!("Library reload failed: {}", e))??;

    Ok(DesktopCleanupReport {
        scanned_shortcuts: 0,
        imported_count: 0,
        moved_to_backup: 0,
        already_in_library: 0,
        skipped_non_game: 0,
        backup_dir: String::new(),
        imported_titles: Vec::new(),
        library,
    })
}

#[tauri::command]
pub(crate) async fn get_steam_mini_profile(
    target_override: Option<String>,
    force_refresh: Option<bool>,
) -> Result<crate::steam_profile::SteamMiniProfile, String> {
    crate::steam_profile::resolve_steam_mini_profile(
        target_override,
        force_refresh.unwrap_or(false),
    )
    .await
}

#[tauri::command]
pub(crate) async fn set_steam_profile_override(
    app: AppHandle,
    target_override: Option<String>,
) -> Result<crate::steam_profile::SteamMiniProfile, String> {
    let clean = target_override
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let app_for_save = app.clone();
    let clean_for_save = clean.clone();
    let _ = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut settings = load_settings_from_disk(&app_for_save);
        settings.steam_id_override = clean_for_save;
        save_settings_to_disk(&app_for_save, &settings)
    })
    .await;
    crate::steam_profile::clear_steam_profile_cache();
    crate::friends::clear_friends_cache();
    let profile = crate::steam_profile::resolve_steam_mini_profile(clean, true).await?;
    emit_if_alive(&app, "steam-profile-updated", &profile);
    Ok(profile)
}

#[tauri::command]
pub(crate) async fn save_steam_id_override(
    app: AppHandle,
    steam_id_override: Option<String>,
) -> Result<crate::steam_profile::SteamMiniProfile, String> {
    set_steam_profile_override(app, steam_id_override).await
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PurgeNonGamesResult {
    pub removed_count: usize,
    pub removed_titles: Vec<String>,
    pub remaining_count: usize,
    pub games: Vec<GameEntry>,
}

pub(crate) fn purge_non_game_entries_from_vec(
    library: &mut Vec<GameEntry>,
) -> Vec<(String, String)> {
    let mut removed = Vec::new();
    library.retain(|g| {
        let is_rom = g.platform_id > 0
            || !g.rom_id.trim().is_empty()
            || !g.emulator_id.trim().is_empty()
            || g.source.eq_ignore_ascii_case("RetroArch");
        let check_exe = if is_rom && !g.exe_path.trim().is_empty() {
            "retro_rom_entry.exe"
        } else {
            g.exe_path.as_str()
        };
        let aid = g
            .app_id
            .or_else(|| g.steam_app_id.and_then(|v| u32::try_from(v).ok()));
        let is_bad = importers::is_non_game_or_dlc_entry(&g.title, aid, &g.install_dir, check_exe);
        if is_bad {
            removed.push((g.id.clone(), g.title.clone()));
            false
        } else {
            true
        }
    });
    removed
}

#[tauri::command]
pub(crate) async fn purge_non_game_entries(app: AppHandle) -> Result<PurgeNonGamesResult, String> {
    let app_for_work = app.clone();
    let res = tokio::task::spawn_blocking(move || -> Result<PurgeNonGamesResult, String> {
        let mut library = load_library_from_disk(&app_for_work)?;
        let removed_pairs = purge_non_game_entries_from_vec(&mut library);
        let removed_count = removed_pairs.len();
        let removed_titles: Vec<String> = removed_pairs.iter().map(|(_, t)| t.clone()).collect();
        if removed_count > 0 {
            save_library_to_disk(&app_for_work, &library)?;
            for (gid, _) in &removed_pairs {
                sidebar_icons::invalidate_sidebar_icon(&app_for_work, gid);
            }
        }
        let remaining_count = library.len();
        Ok(PurgeNonGamesResult {
            removed_count,
            removed_titles,
            remaining_count,
            games: library,
        })
    })
    .await
    .map_err(|e| format!("Failed purging non-game entries: {}", e))??;

    if res.removed_count > 0 {
        emit_if_alive(&app, "library-updated", &res.games);
    }
    Ok(res)
}

pub(crate) fn spawn_background_missing_covers_resolver(app: AppHandle) {
    artwork::spawn_background_cover_worker_pool(app, None);
}

pub(crate) fn spawn_initial_library_auto_scan_if_needed(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(650)).await;
        if is_shutting_down() {
            return;
        }
        let app_check = app.clone();
        let should_scan = tokio::task::spawn_blocking(move || -> bool {
            let settings = load_settings_from_disk(&app_check);
            if settings.auto_initial_scan_done {
                return false;
            }
            let lib = load_library_lightweight(&app_check).unwrap_or_default();
            let mut updated = settings;
            updated.auto_initial_scan_done = true;
            let _ = save_settings_to_disk(&app_check, &updated);
            lib.is_empty()
        })
        .await
        .unwrap_or(false);

        if !should_scan || is_shutting_down() {
            return;
        }

        if let Ok(res) = importers::import_existing_libraries_core().await {
            if !is_shutting_down() {
                emit_if_alive(&app, "library-updated", &res.games);
                emit_if_alive(&app, "initial-library-scan-complete", &res);
                if !res.games.is_empty() {
                    spawn_background_missing_covers_resolver(app.clone());
                }
            }
        }
    });
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct BatchRemoveResult {
    pub removed_count: usize,
    pub deleted_files_count: usize,
    pub save_backups_created: usize,
    pub games: Vec<GameEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct BatchBackupResult {
    pub backed_up_count: usize,
    pub skipped_count: usize,
    pub backup_paths: Vec<String>,
}

#[tauri::command]
pub(crate) async fn batch_remove_games(
    app: AppHandle,
    game_ids: Vec<String>,
    delete_files: Option<bool>,
) -> Result<BatchRemoveResult, String> {
    let target_set: HashSet<String> = game_ids
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if target_set.is_empty() {
        let lib = load_library_from_disk(&app)?;
        return Ok(BatchRemoveResult {
            removed_count: 0,
            deleted_files_count: 0,
            save_backups_created: 0,
            games: lib,
        });
    }

    let mut deleted_files_count = 0usize;
    let mut save_backups_created = 0usize;
    let mut removed_count = 0usize;

    if delete_files.unwrap_or(false) {
        for gid in &target_set {
            match uninstaller::execute_clean_uninstall(app.clone(), gid.clone(), true, Some(false))
                .await
            {
                Ok(report) => {
                    removed_count += 1;
                    deleted_files_count += 1;
                    if report.save_archive_path.is_some() {
                        save_backups_created += 1;
                    }
                }
                Err(_) => {
                    // Fallback to removing the launcher entry if directory was already missing
                    let _ = remove_local_game(app.clone(), gid.clone()).await;
                    removed_count += 1;
                }
            }
        }
        let final_lib = tokio::task::spawn_blocking({
            let app_c = app.clone();
            move || load_library_from_disk(&app_c)
        })
        .await
        .map_err(|e| format!("Failed reloading library: {e}"))??;
        emit_if_alive(&app, "library-updated", &final_lib);
        return Ok(BatchRemoveResult {
            removed_count,
            deleted_files_count,
            save_backups_created,
            games: final_lib,
        });
    }

    let app_work = app.clone();
    let final_lib = tokio::task::spawn_blocking(move || -> Result<(usize, Vec<GameEntry>), String> {
        let mut library = load_library_from_disk(&app_work)?;
        let before = library.len();
        let mut removed_ids = Vec::new();
        library.retain(|g| {
            let hit = target_set.contains(&g.id)
                || target_set.iter().any(|k| g.exe_path.eq_ignore_ascii_case(k));
            if hit {
                removed_ids.push(g.id.clone());
                false
            } else {
                true
            }
        });
        let removed = before.saturating_sub(library.len());
        if removed > 0 {
            save_library_to_disk(&app_work, &library)?;
            for gid in &removed_ids {
                sidebar_icons::invalidate_sidebar_icon(&app_work, gid);
            }
        }
        Ok((removed, library))
    })
    .await
    .map_err(|e| format!("Batch remove worker failed: {e}"))??;

    emit_if_alive(&app, "library-updated", &final_lib.1);
    Ok(BatchRemoveResult {
        removed_count: final_lib.0,
        deleted_files_count: 0,
        save_backups_created: 0,
        games: final_lib.1,
    })
}

#[tauri::command]
pub(crate) async fn batch_fetch_covers(
    app: AppHandle,
    game_ids: Vec<String>,
) -> Result<usize, String> {
    let clean_ids: Vec<String> = game_ids
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let count = clean_ids.len();
    if count > 0 {
        artwork::spawn_background_cover_worker_pool(app, Some(clean_ids));
    }
    Ok(count)
}

#[tauri::command]
pub(crate) async fn batch_backup_saves(
    game_ids: Vec<String>,
) -> Result<BatchBackupResult, String> {
    let mut backed_up_count = 0usize;
    let mut skipped_count = 0usize;
    let mut backup_paths = Vec::new();

    for raw_id in game_ids {
        let gid = raw_id.trim().to_string();
        if gid.is_empty() {
            continue;
        }
        match ludusavi::backup_game_saves_core(Some(gid), None, None, None).await {
            Ok(path) => {
                backed_up_count += 1;
                backup_paths.push(path);
            }
            Err(_) => {
                skipped_count += 1;
            }
        }
    }

    Ok(BatchBackupResult {
        backed_up_count,
        skipped_count,
        backup_paths,
    })
}

#[tauri::command]
pub(crate) async fn batch_toggle_favorite(
    app: AppHandle,
    game_ids: Vec<String>,
    favorite: Option<bool>,
) -> Result<Vec<GameEntry>, String> {
    let target_set: HashSet<String> = game_ids
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let app_work = app.clone();
    let updated_lib = tokio::task::spawn_blocking(move || -> Result<Vec<GameEntry>, String> {
        let mut library = load_library_from_disk(&app_work)?;
        if target_set.is_empty() {
            return Ok(library);
        }
        let target_fav = favorite.unwrap_or_else(|| {
            library
                .iter()
                .filter(|g| target_set.contains(&g.id))
                .any(|g| !g.is_favorite)
        });
        let mut changed = false;
        for entry in &mut library {
            if target_set.contains(&entry.id) && entry.is_favorite != target_fav {
                entry.is_favorite = target_fav;
                changed = true;
            }
        }
        if changed {
            save_library_to_disk(&app_work, &library)?;
        }
        Ok(library)
    })
    .await
    .map_err(|e| format!("Batch favorite worker failed: {e}"))??;

    emit_if_alive(&app, "library-updated", &updated_lib);
    Ok(updated_lib)
}




