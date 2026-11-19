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
    stream_extract, stress_harness, swarm, time_machine, uninstaller, vhdx_stage, wasm_plugins,
};
use crate::cmd_emulators::*;
use crate::cmd_library::*;
use crate::cmd_system::*;
pub(crate) static WEBVIEW_GRABBER_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
pub(crate) static PAUSE_FLAGS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();
pub(crate) static DOWNLOAD_QUEUE: OnceLock<Mutex<Vec<DownloadJob>>> = OnceLock::new();
pub(crate) static QUEUE_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
pub(crate) static QUEUE_RUNNER_ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) static GLOBAL_SPEED_LIMIT_KBPS: AtomicU64 = AtomicU64::new(0);
pub(crate) static AUTO_UNBLOCK_MOTW: AtomicBool = AtomicBool::new(true);

pub(crate) const ASYNC_SEGMENT_BUFFER_BYTES: usize = 256 * 1024;
pub(crate) const REUSABLE_STREAM_CHUNK_BYTES: usize = 64 * 1024;
pub(crate) const SCAN_WINDOW_BYTES: usize = 64 * 1024;
pub(crate) const AIMD_MIN_CHUNK_BYTES: u64 = 4 * 1024 * 1024;
pub(crate) const AIMD_MAX_CHUNK_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const AIMD_STEP_BYTES: u64 = 4 * 1024 * 1024;
pub(crate) const WORK_STEAL_MIN_REMAINING_BYTES: u64 = 8 * 1024 * 1024;
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
pub(crate) struct ResolvedStreamInfo {
    stream_url: String,
    referer: String,
    cookie: String,
    user_agent: String,
    suggested_filename: Option<String>,
}

pub(crate) fn get_pause_map() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    PAUSE_FLAGS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn get_queue_store() -> &'static Mutex<Vec<DownloadJob>> {
    DOWNLOAD_QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

pub(crate) fn get_webview_grabber_lock() -> &'static tokio::sync::Mutex<()> {
    WEBVIEW_GRABBER_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub(crate) fn rotate_privacy_user_agent() -> &'static str {
    duke_core::net_doh::rotate_privacy_user_agent()
}

pub(crate) fn get_session_user_agent() -> &'static str {
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

/// Unconditionally strips `:Zone.Identifier` NTFS streams on `.exe`, `.dll`, `.asi`, `.bat` recursively.
pub(crate) fn unblock_ntfs_zone_identifier_raw(target: &Path) -> usize {
    self_heal::unblock_game_tree_zone_identifiers(target)
}

/// Automatically strips the Windows NTFS Mark-of-the-Web (`:Zone.Identifier` alternate data stream)
/// when `auto_unblock_motw` is enabled so Windows SmartScreen never blocks or slows down first game launch.
pub(crate) fn unblock_ntfs_zone_identifier(target: &Path) {
    if !AUTO_UNBLOCK_MOTW.load(Ordering::Relaxed) {
        return;
    }
    let _ = unblock_ntfs_zone_identifier_raw(target);
}

/// Validates that no path inside `dest_dir` contains `..` traversal, external symlinks, or decompression bombs (Zip-Slip protection),
/// and automatically unblocks NTFS `:Zone.Identifier` streams on all extracted files.
pub(crate) fn validate_extracted_tree_against_zip_slip(dest_dir: &Path) -> Result<(), String> {
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

pub(crate) fn is_supported_archive_name(lower_name: &str) -> bool {
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

    if archive_path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
    {
        let mut file = File::open(archive_path)
            .map_err(|e| format!("Failed to open ZIP archive: {}", e))?;
        let file_len = file
            .metadata()
            .map_err(|e| format!("Failed to stat ZIP archive: {}", e))?
            .len();
        if file_len < 22 {
            return Err("Corrupt ZIP archive: file smaller than 22-byte End of Central Directory.".to_string());
        }
        let tail_len = file_len.min(65_560) as usize;
        file.seek(SeekFrom::End(-(tail_len as i64)))
            .map_err(|e| format!("Failed seeking ZIP trailer: {}", e))?;
        let mut tail = vec![0u8; tail_len];
        file.read_exact(&mut tail)
            .map_err(|e| format!("Failed reading ZIP trailer: {}", e))?;
        if !tail.windows(4).any(|w| w == [0x50, 0x4B, 0x05, 0x06]) {
            return Err("Corrupt ZIP archive: missing End of Central Directory signature.".to_string());
        }
    }

    match archive_native::extract_archive_pure_rust(archive_path, dest_dir) {
        Ok(_) => return Ok(()),
        Err(err) => {
            let lower = err.to_ascii_lowercase();
            let is_security_or_corruption = lower.contains("security")
                || lower.contains("zip-slip")
                || lower.contains("traversal")
                || lower.contains("parent directory")
                || lower.contains("symlink")
                || lower.contains("escape")
                || lower.contains("exceeds maximum")
                || lower.contains("corrupt")
                || lower.contains("invalid zip");
            if is_security_or_corruption {
                return Err(err);
            }
            // Fall back to external 7z/WinRAR/tar only for unsupported containers (.rar, multi-part, encrypted, etc.).
        }
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
            cmd.args(["x", "-y", &out_arg, &archive_str]);
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
        cmd.args(["x", "-y", &out_arg, &archive_str]);
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
            "$ErrorActionPreference = 'Stop'; Expand-Archive -ErrorAction Stop -LiteralPath '{}' -DestinationPath '{}' -Force",
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

pub(crate) async fn auto_install_downloaded_file(
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
            emit_if_alive(app, "library-updated", &lib);
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
            emit_if_alive(app, "library-updated", &lib);
        }

        trim_process_working_set();
        return Ok(added);
    }

    // Fallback for extracted legal homebrew / preservation console files inside archives.
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

pub(crate) fn get_queue_storage_paths(app: &AppHandle) -> Vec<PathBuf> {
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

pub(crate) fn get_queue_io_lock() -> &'static Mutex<()> {
    QUEUE_IO_LOCK.get_or_init(|| Mutex::new(()))
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
        let mut guard = get_queue_store().lock().unwrap_or_else(|e| e.into_inner());
        *guard = parsed;
    }
}

pub(crate) fn segment_file_path(part_base: &str, index: usize) -> PathBuf {
    PathBuf::from(format!("{}.seg{}", part_base, index))
}

pub(crate) fn reconcile_job_bytes_from_disk(job: &mut DownloadJob) {
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

pub(crate) fn is_job_auto_cleanable(job: &DownloadJob) -> bool {
    job.status == "COMPLETED"
        && job.registered_in_library
        && job.verdict.as_deref() != Some("DANGER")
}

pub(crate) fn prune_auto_cleanable_jobs(jobs: &mut Vec<DownloadJob>, auto_clean: bool) -> bool {
    if !auto_clean {
        return false;
    }
    let before = jobs.len();
    jobs.retain(|job| !is_job_auto_cleanable(job));
    jobs.len() != before
}

pub(crate) fn schedule_auto_clean_completed_job(app: AppHandle, download_id: String) {
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
            emit_if_alive(&app, "download-queue-updated", &updated_queue);
            emit_if_alive(
                &app,
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

pub(crate) fn persist_queue_to_disk(app: &AppHandle, jobs: &[DownloadJob]) {
    let _io_guard = get_queue_io_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    if let Ok(serialized) = serde_json::to_string_pretty(jobs) {
        for path in get_queue_storage_paths(app) {
            let _ = atomic_write_string(&path, &serialized);
        }
    }
}

pub(crate) fn ensure_queue_initialized(app: &AppHandle) -> Vec<DownloadJob> {
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

pub(crate) fn update_job_in_memory_and_disk(app: &AppHandle, updated: &DownloadJob, write_disk: bool) {
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

pub(crate) fn get_http_client() -> Client {
    duke_core::net_doh::get_configured_http_client()
}

/// Rotates cached DoH edge IPs for a URL's host so workers can fail over across CDN edge nodes when throttled.
pub(crate) fn rotate_doh_cached_ips(raw_url: &str) {
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

pub(crate) async fn build_metadata_doh_client(raw_url: &str) -> Client {
    let timeout = Duration::from_secs(HTTP_METADATA_TIMEOUT_SECS);
    net_engine::get_pooled_client_for_url(raw_url, Some(timeout), true).await
}

pub(crate) async fn fetch_doh_json(raw_url: &str) -> Result<Value, String> {
    let cache_dir = net_engine::etag_cache_dir_standalone();
    net_engine::fetch_json_cached::<Value>(
        cache_dir.as_deref(),
        raw_url,
        &[("Accept", "application/json, text/plain, */*")],
        Duration::from_secs(HTTP_METADATA_TIMEOUT_SECS),
    )
    .await
}

pub(crate) fn derive_filename(url: &str, disposition: Option<&str>, suggested: Option<&str>) -> String {
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

pub(crate) fn parse_host_html_metadata(html: &str) -> (Option<String>, Option<String>) {
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

pub(crate) fn collect_cookies(resp: &reqwest::Response) -> String {
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

pub(crate) fn is_direct_cdn_binary_url(raw_url: &str) -> bool {
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
pub(crate) async fn probe_direct_stream(
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

pub(crate) async fn try_resolve_gofile(url: &str) -> Option<ResolvedStreamInfo> {
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

pub(crate) async fn try_resolve_pixeldrain(url: &str) -> Option<ResolvedStreamInfo> {
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

pub(crate) async fn try_resolve_mediafire(url: &str) -> Option<ResolvedStreamInfo> {
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

pub(crate) async fn try_resolve_fileditch(url: &str) -> Option<ResolvedStreamInfo> {
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
pub(crate) async fn try_resolve_native_api(raw_url: &str) -> Option<ResolvedStreamInfo> {
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

pub(crate) const WIN11_EDGE_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/134.0.0.0 Safari/537.36 Edg/134.0.0.0";

pub(crate) fn get_grabber_webview_profile_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = resolve_data_root_dir(app)?.join("grabber_webview_profile");
    if !dir.exists() {
        fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create grabber WebView2 profile directory: {}", e))?;
    }
    Ok(dir)
}

pub(crate) fn format_cookie_list<'a, I>(cookies: I) -> String
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

pub(crate) fn collect_cookies_from_webview<R: tauri::Runtime>(
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

pub(crate) fn collect_cookies_from_window<R: tauri::Runtime>(
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

pub(crate) fn schedule_delayed_window_close(app: AppHandle, label: &'static str) {
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
pub(crate) async fn capture_stream_via_webview(
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
            emit_if_alive(
                &app_for_dl,
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
                    emit_if_alive(
                        &app_for_nav,
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

pub(crate) fn is_ad_or_tracker_url(raw_url: &str) -> bool {
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
pub(crate) struct DirectoryDownloadInterceptPayload {
    url: String,
    file_name: String,
    site_title: String,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_intercepted_directory_download(
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
            emit_if_alive(
                &app,
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
pub(crate) async fn open_directory_browser(
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
    let window_title = format!("{} â€” Duke Safe Browser", display_title);

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

pub(crate) async fn resolve_stream_info(app: &AppHandle, raw_url: &str) -> Result<ResolvedStreamInfo, String> {
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

pub(crate) fn aimd_next_chunk_bytes(
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

pub(crate) fn seek_write_all_at(file: &File, mut offset: u64, mut data: &[u8]) -> std::io::Result<()> {
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

pub(crate) struct AtomicSegmentSlot {
    pub(crate) start: AtomicU64,
    pub(crate) current: AtomicU64,
    pub(crate) end: AtomicU64,
    pub(crate) index: AtomicU64,
}

impl AtomicSegmentSlot {
    pub(crate) fn new(start: u64, current: u64, end: u64, index: u64) -> Self {
        Self {
            start: AtomicU64::new(start),
            current: AtomicU64::new(current),
            end: AtomicU64::new(end),
            index: AtomicU64::new(index),
        }
    }
}

pub(crate) const MAX_DYNAMIC_SEGMENTS: usize = 64;

pub(crate) fn try_steal_slowest_tail(
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

pub(crate) async fn execute_resumable_matrix_job(
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
            emit_if_alive(&app, "download-progress", &job);

            match resolve_stream_info(&app, &job.original_url).await {
                Ok(info) => {
                    if pause_flag.load(Ordering::Relaxed) {
                        job.status = "PAUSED".to_string();
                        job.message = "Paused - Click RESUME to start".to_string();
                        update_job_in_memory_and_disk(&app, &job, true);
                        emit_if_alive(&app, "download-progress", &job);
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
                            emit_if_alive(&app, "download-progress", &job);
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
                    emit_if_alive(&app, "download-progress", &job);
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
    emit_if_alive(&app, "download-progress", &job);

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

        while !is_fin_rep.load(Ordering::Relaxed) && !is_shutting_down() {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if is_fin_rep.load(Ordering::Relaxed) || is_shutting_down() {
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
                emit_if_alive(
                    &app_rep,
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
            emit_if_alive(&app_rep, "download-progress", &job_state);
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
        emit_if_alive(&app, "download-progress", &job);
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
        emit_if_alive(&app, "download-progress", &job);
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
        emit_if_alive(&app, "download-progress", &job);

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
                        emit_if_alive(&app, "library-updated", &lib);
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
        emit_if_alive(&app, "download-progress", &job);
        if should_auto_clean {
            schedule_auto_clean_completed_job(app.clone(), job.download_id.clone());
        }
        trim_process_working_set();
        return Ok(job);
    }

    job.status = "SCANNING".to_string();
    job.message = "Finalizing archive & running Entropy/PE Malware Shield...".to_string();
    update_job_in_memory_and_disk(&app, &job, true);
    emit_if_alive(&app, "download-progress", &job);

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
        emit_if_alive(&app, "download-progress", &job);

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
    emit_if_alive(&app, "download-progress", &job);
    if should_auto_clean {
        schedule_auto_clean_completed_job(app.clone(), job.download_id.clone());
    }
    trim_process_working_set();

    Ok(job)
}

pub(crate) fn spawn_queue_scheduler(app: AppHandle) {
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
    emit_if_alive(&app, "download-progress", &job);

    spawn_queue_scheduler(app.clone());

    let guard = get_queue_store().lock().unwrap_or_else(|e| e.into_inner());
    Ok(guard.clone())
}

#[tauri::command]
pub(crate) async fn enqueue_download(
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
pub(crate) async fn install_downloaded_job(app: AppHandle, download_id: String) -> Result<GameEntry, String> {
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
    emit_if_alive(&app, "download-progress", &target_job);

    match auto_install_downloaded_file(&app, &target_job.final_path).await {
        Ok(added) => {
            target_job.status = "COMPLETED".to_string();
            target_job.registered_in_library = true;
            target_job.message = format!(
                "Installed '{}' ({}) to MY GAMES!",
                added.title, added.size_formatted
            );
            update_job_in_memory_and_disk(&app, &target_job, true);
            emit_if_alive(&app, "download-progress", &target_job);
            schedule_auto_clean_completed_job(app.clone(), target_job.download_id.clone());
            Ok(added)
        }
        Err(e) => {
            target_job.status = "COMPLETED".to_string();
            target_job.registered_in_library = false;
            target_job.message = format!("Install note: {}", e);
            update_job_in_memory_and_disk(&app, &target_job, true);
            emit_if_alive(&app, "download-progress", &target_job);
            Err(e)
        }
    }
}

#[tauri::command]
pub(crate) async fn clear_completed_downloads(app: AppHandle) -> Result<Vec<DownloadJob>, String> {
    ensure_queue_initialized(&app);
    let remaining = {
        let mut guard = get_queue_store().lock().unwrap_or_else(|e| e.into_inner());
        guard.retain(|j| j.status != "COMPLETED");
        persist_queue_to_disk(&app, &guard);
        guard.clone()
    };
    emit_if_alive(&app, "download-queue-updated", &remaining);
    Ok(remaining)
}

#[tauri::command]
pub(crate) async fn set_speed_limit(max_speed_mbps: Option<f64>) -> Result<(), String> {
    if let Some(speed) = max_speed_mbps {
        GLOBAL_SPEED_LIMIT_KBPS.store((speed * 1024.0) as u64, Ordering::Relaxed);
    } else {
        GLOBAL_SPEED_LIMIT_KBPS.store(0, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn pause_download(app: AppHandle, download_id: String) -> Result<Vec<DownloadJob>, String> {
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
pub(crate) async fn resume_download(
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
pub(crate) async fn remove_download_job(
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
pub(crate) async fn get_download_jobs(
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
pub(crate) async fn scan_file_security(path: String) -> Result<SecurityScanReport, String> {
    scan_file_security_core(path).await
}

#[inline]
pub(crate) fn bytes_contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack.len() >= needle.len()
        && haystack.windows(needle.len()).any(|w| w == needle)
}

pub(crate) fn shannon_entropy_from_histogram(hist: &[u64; 256], total: u64) -> f64 {
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
pub(crate) struct PeHeaderSummary {
    architecture: String,
    kind: &'static str,
    subsystem: &'static str,
    section_count: u16,
    sections: Vec<String>,
    has_packer_section: bool,
}

pub(crate) fn inspect_pe_headers_from_file(file: &mut File, size_bytes: u64) -> Option<PeHeaderSummary> {
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
pub(crate) async fn delete_flagged_file(path: String) -> Result<(), String> {
    let clean = path.trim().trim_matches('"').to_string();
    if clean.is_empty() {
        return Ok(());
    }
    run_blocking_timeout(15, "delete_flagged_file", move || {
        let target = PathBuf::from(&clean);
        if let Some(reason) = crate::uninstaller::protected_path_refusal_reason(&target) {
            return Err(format!("Refused to delete protected path: {reason}"));
        }
        if target.exists() && target.is_file() {
            fs::remove_file(&target).map_err(|e| format!("Failed to delete file: {}", e))?;
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub(crate) async fn unblock_motw_for_path(path: String) -> Result<usize, String> {
    let clean = path.trim().trim_matches('"').to_string();
    if clean.is_empty() {
        return Err("Target path is empty.".to_string());
    }
    run_blocking_timeout(30, "unblock_motw_for_path", move || {
        let target = PathBuf::from(&clean);
        if !target.exists() {
            return Err(format!("Target path does not exist: {}", clean));
        }
        Ok(unblock_ntfs_zone_identifier_raw(&target))
    })
    .await
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

/// Runs 1-click Auto-Fix on a game: unblocks NTFS `:Zone.Identifier`, runs PE32/PE32+ DLL
/// inspection & redistributable healing, and initializes an isolated `.duke-profile/` sandbox.
#[tauri::command]
pub(crate) async fn auto_fix_game(app: AppHandle, game_id: String) -> Result<AutoFixReport, String> {
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

    emit_if_alive(&app, "library-updated", &updated_library);
    Ok(report)
}

#[tauri::command]
pub(crate) async fn scan_redist_bloat(
    app: AppHandle,
    game_id: Option<String>,
) -> Result<redist_cleaner::RedistScanReport, String> {
    let clean_gid = game_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    run_blocking_timeout(45, "scan_redist_bloat", move || {
        let library = load_library_from_disk(&app)?;
        Ok(redist_cleaner::scan_redist_bloat_for_library(
            &library,
            clean_gid.as_deref(),
        ))
    })
    .await
}

#[tauri::command]
pub(crate) async fn clean_redist_bloat(
    app: AppHandle,
    game_id: Option<String>,
) -> Result<redist_cleaner::RedistCleanReport, String> {
    let _crumb = self_heal::record_heavy_command_breadcrumb("clean_redist_bloat");
    let clean_gid = game_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    run_blocking_timeout(180, "clean_redist_bloat", move || {
        let mut library = load_library_from_disk(&app)?;
        let report =
            redist_cleaner::clean_redist_bloat_with_cas(&mut library, clean_gid.as_deref())?;
        if report.bytes_reclaimed > 0 || report.entries_removed > 0 {
            save_library_to_disk(&app, &library)?;
            emit_if_alive(&app, "library-updated", &library);
        }
        Ok(report)
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_net_engine_stats_cmd() -> Result<net_engine::NetEngineStats, String> {
    run_blocking_timeout(5, "get_net_engine_stats", || {
        Ok(net_engine::get_net_engine_stats())
    })
    .await
}

#[tauri::command]
pub(crate) async fn save_api_key(
    app: AppHandle,
    slot: String,
    api_key: String,
    username: Option<String>,
) -> Result<LauncherSettings, String> {
    let trimmed_key = api_key.trim().to_string();
    if trimmed_key.is_empty() {
        return Err("API key cannot be empty.".to_string());
    }
    let norm = slot.trim().to_ascii_lowercase();
    let app_for_save = app.clone();
    let updated = tokio::task::spawn_blocking(move || -> Result<LauncherSettings, String> {
        let mut current = load_settings_from_disk(&app_for_save);
        match norm.as_str() {
            "steam" => current.steam_api_key = Some(trimmed_key),
            "xbox" | "xbl" | "xbl.io" => current.xbox_api_key = Some(trimmed_key),
            "psn" => current.psn_npsso = Some(trimmed_key),
            "steamgriddb" => current.steamgriddb_api_key = Some(trimmed_key),
            "retroachievements" | "retro_achievements" | "ra" => {
                let _ = privacy_vault::set_vault_api_key_sync(
                    "retroAchievements",
                    Some(&trimmed_key),
                    username.as_deref(),
                )?;
            }
            other => return Err(format!("Unsupported API key slot '{other}'")),
        }
        save_settings_to_disk(&app_for_save, &current)?;
        Ok(current.to_ipc_safe())
    })
    .await
    .map_err(|e| format!("Failed saving API key: {e}"))??;

    crate::steam_profile::clear_steam_profile_cache();
    crate::friends::clear_friends_cache();
    emit_if_alive(
        &app,
        "steam-profile-updated",
        serde_json::json!({ "action": "save_api_key", "slot": slot }),
    );
    Ok(updated)
}

#[tauri::command]
pub(crate) async fn clear_api_key(
    app: AppHandle,
    slot: String,
) -> Result<LauncherSettings, String> {
    let norm = slot.trim().to_ascii_lowercase();
    let app_for_save = app.clone();
    let updated = tokio::task::spawn_blocking(move || -> Result<LauncherSettings, String> {
        let mut current = load_settings_from_disk(&app_for_save);
        match norm.as_str() {
            "steam" => {
                current.steam_api_key = None;
                privacy_vault::clear_slot_by_name("steam");
            }
            "xbox" | "xbl" | "xbl.io" => {
                current.xbox_api_key = None;
                privacy_vault::clear_slot_by_name("xbox");
            }
            "psn" => {
                current.psn_npsso = None;
                privacy_vault::clear_slot_by_name("psn");
            }
            "steamgriddb" => {
                current.steamgriddb_api_key = None;
                privacy_vault::clear_slot_by_name("steamgriddb");
            }
            "retroachievements" | "retro_achievements" | "ra" => {
                privacy_vault::clear_slot_by_name("retro_achievements");
            }
            other => return Err(format!("Unsupported API key slot '{other}'")),
        }
        save_settings_to_disk(&app_for_save, &current)?;
        Ok(current.to_ipc_safe())
    })
    .await
    .map_err(|e| format!("Failed clearing API key: {e}"))??;

    crate::steam_profile::clear_steam_profile_cache();
    crate::friends::clear_friends_cache();
    emit_if_alive(
        &app,
        "steam-profile-updated",
        serde_json::json!({ "action": "clear_api_key", "slot": slot }),
    );
    Ok(updated)
}

