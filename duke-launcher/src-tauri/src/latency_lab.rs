use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

use duke_core::nvme_prefetch::windows_build_number;
use duke_core::win32_ffi::{
    detect_primary_gpu_driver_version, inspect_frame_pacing_status, reg_delete_value,
    reg_enum_string_values, reg_read_dword, reg_read_string, reg_write_string,
    vendor_shader_cache_dirs,
};

use crate::{atomic_write_string, format_bytes, get_app_data_file_standalone};

const LATENCY_PROFILES_FILE: &str = "latency_lab_profiles.json";
const REG_USER_GPU_PREFS: &str = r"Software\Microsoft\DirectX\UserGpuPreferences";
const REG_DWM: &str = r"SOFTWARE\Microsoft\Windows\Dwm";
const REG_GRAPHICS_DRIVERS: &str = r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LatencyLabGameProfile {
    pub exe_path: String,
    pub gpu_preference: u8,         // 0 = Windows Decide, 1 = Power Saving, 2 = High Performance
    pub flip_model_upgrade: bool,   // SwapEffectUpgradeEnable=1
    pub auto_hdr_enable: bool,      // AutoHDREnable=1
    pub auto_sr_mode: String,       // "off" | "1080p" | "undocked" | "native_auto"
    pub updated_at_epoch_sec: u64,
}

impl LatencyLabGameProfile {
    pub fn default_for_exe(exe_path: &str) -> Self {
        Self {
            exe_path: exe_path.to_string(),
            gpu_preference: 2,
            flip_model_upgrade: true,
            auto_hdr_enable: false,
            auto_sr_mode: "off".to_string(),
            updated_at_epoch_sec: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LatencyLabStore {
    pub last_seen_driver_version: String,
    pub gpu_adapter_name: String,
    pub shader_cache_cap_mb: u64,
    pub guard_reapply_count: u64,
    pub last_reapply_epoch_sec: u64,
    pub driver_changed_last_boot: bool,
    pub profiles: HashMap<String, LatencyLabGameProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MpoDiagnosticStatus {
    pub mpo_enabled: bool,
    pub overlay_test_mode: Option<u32>,
    pub disable_overlays_reg: Option<u32>,
    pub dxgi_outputs_detected: usize,
    pub windows_build: u32,
    pub timer_resolution_ms: f64,
    pub recommendation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderCacheBucket {
    pub label: String,
    pub path: String,
    pub size_bytes: u64,
    pub size_formatted: String,
    pub file_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyLabSystemStatus {
    pub driver_version: String,
    pub gpu_name: String,
    pub gpu_vendor: String,
    pub driver_changed_last_boot: bool,
    pub guard_reapply_count: u64,
    pub saved_profile_count: usize,
    pub registry_entry_count: usize,
    pub shader_cache_cap_mb: u64,
    pub shader_cache_total_bytes: u64,
    pub shader_cache_total_formatted: String,
    pub shader_cache_buckets: Vec<ShaderCacheBucket>,
    pub mpo: MpoDiagnosticStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverGuardSyncReport {
    pub driver_changed: bool,
    pub previous_driver: String,
    pub current_driver: String,
    pub reapplied_count: usize,
    pub failed_count: usize,
    pub guard_reapply_total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderCacheTrimReport {
    pub cap_mb: u64,
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub freed_bytes: u64,
    pub freed_formatted: String,
    pub files_pruned: usize,
}

fn now_epoch_sec() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn normalize_exe_key(exe_path: &str) -> String {
    exe_path.trim().to_ascii_lowercase()
}

pub(crate) fn load_latency_store() -> LatencyLabStore {
    let mut store: LatencyLabStore = match get_app_data_file_standalone(LATENCY_PROFILES_FILE) {
        Ok(path) => fs::read_to_string(path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default(),
        Err(_) => LatencyLabStore::default(),
    };
    if store.shader_cache_cap_mb == 0 {
        store.shader_cache_cap_mb = 4096;
    }
    store
}

pub(crate) fn save_latency_store(store: &LatencyLabStore) -> Result<(), String> {
    let path = get_app_data_file_standalone(LATENCY_PROFILES_FILE)?;
    let json = serde_json::to_string_pretty(store)
        .map_err(|e| format!("Failed to serialize Latency Lab store: {}", e))?;
    atomic_write_string(&path, &json)
}

pub fn normalize_auto_sr_mode(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1080p" | "fhd" | "1920x1080" => "1080p".to_string(),
        "undocked" | "handheld" | "efficiency" => "undocked".to_string(),
        "1" | "on" | "auto" | "native_auto" => "native_auto".to_string(),
        _ => "off".to_string(),
    }
}

pub fn parse_user_gpu_preferences(exe_path: &str, raw: &str) -> LatencyLabGameProfile {
    let mut profile = LatencyLabGameProfile::default_for_exe(exe_path);
    profile.gpu_preference = 0;
    profile.flip_model_upgrade = false;

    let mut auto_sr_enabled = false;
    let mut auto_sr_profile_hint = String::new();
    let mut auto_sr_res_hint = String::new();

    for token in raw.split(';') {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((k, v)) = trimmed.split_once('=') else {
            continue;
        };
        let key = k.trim();
        let val = v.trim();
        if key.eq_ignore_ascii_case("GpuPreference") {
            if let Ok(n) = val.parse::<u8>() {
                profile.gpu_preference = n.min(2);
            }
        } else if key.eq_ignore_ascii_case("SwapEffectUpgradeEnable") {
            profile.flip_model_upgrade = val == "1";
        } else if key.eq_ignore_ascii_case("AutoHDREnable") {
            profile.auto_hdr_enable = val == "1";
        } else if key.eq_ignore_ascii_case("AutoSREnable") {
            auto_sr_enabled = val == "1";
        } else if key.eq_ignore_ascii_case("AutoSRProfile") {
            auto_sr_profile_hint = val.to_string();
        } else if key.eq_ignore_ascii_case("AutoSRResolution") {
            auto_sr_res_hint = val.to_string();
        }
    }

    profile.auto_sr_mode = if !auto_sr_enabled {
        "off".to_string()
    } else if auto_sr_profile_hint.eq_ignore_ascii_case("undocked")
        || auto_sr_res_hint.eq_ignore_ascii_case("1280x720")
    {
        "undocked".to_string()
    } else if auto_sr_res_hint.eq_ignore_ascii_case("1920x1080")
        || auto_sr_profile_hint.eq_ignore_ascii_case("1080p")
    {
        "1080p".to_string()
    } else {
        "native_auto".to_string()
    };

    profile
}

pub fn format_user_gpu_preferences(
    existing_raw: Option<&str>,
    profile: &LatencyLabGameProfile,
) -> String {
    let mut extras: Vec<(String, String)> = Vec::new();
    if let Some(raw) = existing_raw {
        for token in raw.split(';') {
            let trimmed = token.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some((k, v)) = trimmed.split_once('=') {
                let key = k.trim();
                let managed = key.eq_ignore_ascii_case("GpuPreference")
                    || key.eq_ignore_ascii_case("SwapEffectUpgradeEnable")
                    || key.eq_ignore_ascii_case("AutoHDREnable")
                    || key.eq_ignore_ascii_case("AutoSREnable")
                    || key.eq_ignore_ascii_case("AutoSRProfile")
                    || key.eq_ignore_ascii_case("AutoSRResolution");
                if !managed {
                    extras.push((key.to_string(), v.trim().to_string()));
                }
            }
        }
    }

    let mut pairs: Vec<String> = Vec::with_capacity(8 + extras.len());
    pairs.push(format!("GpuPreference={}", profile.gpu_preference.min(2)));
    pairs.push(format!(
        "SwapEffectUpgradeEnable={}",
        if profile.flip_model_upgrade { 1 } else { 0 }
    ));
    pairs.push(format!(
        "AutoHDREnable={}",
        if profile.auto_hdr_enable { 1 } else { 0 }
    ));

    match normalize_auto_sr_mode(&profile.auto_sr_mode).as_str() {
        "1080p" => {
            pairs.push("AutoSREnable=1".to_string());
            pairs.push("AutoSRProfile=1080p".to_string());
            pairs.push("AutoSRResolution=1920x1080".to_string());
        }
        "undocked" => {
            pairs.push("AutoSREnable=1".to_string());
            pairs.push("AutoSRProfile=undocked".to_string());
            pairs.push("AutoSRResolution=1280x720".to_string());
        }
        "native_auto" => {
            pairs.push("AutoSREnable=1".to_string());
            pairs.push("AutoSRProfile=auto".to_string());
        }
        _ => {
            pairs.push("AutoSREnable=0".to_string());
        }
    }

    for (k, v) in extras {
        pairs.push(format!("{}={}", k, v));
    }

    let mut out = pairs.join(";");
    out.push(';');
    out
}

pub fn write_exe_registry_preferences(profile: &LatencyLabGameProfile) -> Result<String, String> {
    let clean_exe = profile.exe_path.trim();
    if clean_exe.is_empty() {
        return Err("Executable path cannot be empty for UserGpuPreferences.".to_string());
    }
    let existing = reg_read_string(false, REG_USER_GPU_PREFS, clean_exe);
    let formatted = format_user_gpu_preferences(existing.as_deref(), profile);
    reg_write_string(false, REG_USER_GPU_PREFS, clean_exe, &formatted)?;
    Ok(formatted)
}

pub fn read_exe_registry_preferences(exe_path: &str) -> LatencyLabGameProfile {
    let clean_exe = exe_path.trim();
    if let Some(raw) = reg_read_string(false, REG_USER_GPU_PREFS, clean_exe) {
        return parse_user_gpu_preferences(clean_exe, &raw);
    }
    let store = load_latency_store();
    store
        .profiles
        .get(&normalize_exe_key(clean_exe))
        .cloned()
        .unwrap_or_else(|| LatencyLabGameProfile::default_for_exe(clean_exe))
}

pub fn set_auto_hdr_for_exe(exe_path: &str, enabled: bool) -> Result<LatencyLabGameProfile, String> {
    let mut store = load_latency_store();
    let key = normalize_exe_key(exe_path);
    let mut profile = store
        .profiles
        .get(&key)
        .cloned()
        .unwrap_or_else(|| read_exe_registry_preferences(exe_path));
    profile.exe_path = exe_path.trim().to_string();
    profile.auto_hdr_enable = enabled;
    profile.updated_at_epoch_sec = now_epoch_sec();

    write_exe_registry_preferences(&profile)?;
    store.profiles.insert(key, profile.clone());
    save_latency_store(&store)?;
    Ok(profile)
}

pub fn apply_saved_profile_if_present(exe_path: &str) {
    let store = load_latency_store();
    if let Some(profile) = store.profiles.get(&normalize_exe_key(exe_path)) {
        let _ = write_exe_registry_preferences(profile);
    }
}

pub fn shader_cache_launch_env() -> Vec<(String, String)> {
    let store = load_latency_store();
    let cap_mb = store.shader_cache_cap_mb.clamp(512, 32768);
    let cap_bytes = cap_mb.saturating_mul(1024 * 1024);
    vec![
        (
            "__GL_SHADER_DISK_CACHE_SIZE".to_string(),
            cap_bytes.to_string(),
        ),
        (
            "AMD_SHADER_DISK_CACHE_MAX_SIZE".to_string(),
            cap_bytes.to_string(),
        ),
    ]
}

fn count_dxgi_outputs() -> usize {
    #[cfg(windows)]
    {
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
        let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
            return 0;
        };
        let mut total = 0usize;
        let mut adapter_idx = 0u32;
        while let Ok(adapter) = unsafe { factory.EnumAdapters1(adapter_idx) } {
            let mut out_idx = 0u32;
            while unsafe { adapter.EnumOutputs(out_idx) }.is_ok() {
                total += 1;
                out_idx += 1;
            }
            adapter_idx += 1;
        }
        total
    }
    #[cfg(not(windows))]
    {
        0
    }
}

pub fn probe_mpo_diagnostics() -> MpoDiagnosticStatus {
    let overlay_test_mode = reg_read_dword(true, REG_DWM, "OverlayTestMode");
    let disable_overlays_reg = reg_read_dword(true, REG_GRAPHICS_DRIVERS, "DisableOverlays");
    let pacing = inspect_frame_pacing_status();
    let dxgi_outputs = count_dxgi_outputs();

    let mpo_disabled_by_dwm = overlay_test_mode == Some(5);
    let mpo_disabled_by_driver = disable_overlays_reg.unwrap_or(0) != 0;
    let mpo_enabled = !mpo_disabled_by_dwm && !mpo_disabled_by_driver;

    let recommendation = if mpo_disabled_by_dwm {
        "MPO disabled via OverlayTestMode=5. Flip-Model still bypasses DWM in Borderless Fullscreen; keep MPO off if experiencing multi-monitor flicker.".to_string()
    } else if mpo_disabled_by_driver {
        "MPO disabled via GraphicsDrivers DisableOverlays.".to_string()
    } else if dxgi_outputs > 1 {
        "MPO Active across multiple displays: ideal for low-latency independent flip queues; toggle OverlayTestMode only if VRR flickering appears.".to_string()
    } else {
        "MPO Active on primary display: optimal zero-copy independent flip path.".to_string()
    };

    MpoDiagnosticStatus {
        mpo_enabled,
        overlay_test_mode,
        disable_overlays_reg,
        dxgi_outputs_detected: dxgi_outputs,
        windows_build: windows_build_number(),
        timer_resolution_ms: (pacing.actual_100ns_units as f64) / 10_000.0,
        recommendation,
    }
}

fn measure_shader_cache_buckets() -> Vec<ShaderCacheBucket> {
    let dirs = vendor_shader_cache_dirs();
    let mut buckets = Vec::with_capacity(dirs.len());
    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }
        let label = dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("ShaderCache")
            .to_string();
        let mut size_bytes = 0u64;
        let mut file_count = 0usize;
        for entry in WalkDir::new(&dir).follow_links(false).into_iter().flatten() {
            if entry.file_type().is_file() {
                file_count += 1;
                size_bytes = size_bytes.saturating_add(entry.metadata().map(|m| m.len()).unwrap_or(0));
            }
        }
        buckets.push(ShaderCacheBucket {
            label,
            path: dir.to_string_lossy().to_string(),
            size_bytes,
            size_formatted: format_bytes(size_bytes),
            file_count,
        });
    }
    buckets
}

pub fn run_driver_profile_guard_internal(force_reapply: bool) -> Result<DriverGuardSyncReport, String> {
    let gpu = crate::shader_swarm::detect_gpu_hardware_identity();
    let live_ver = detect_primary_gpu_driver_version().unwrap_or_else(|| gpu.driver_version.clone());

    let mut store = load_latency_store();
    let prev = store.last_seen_driver_version.clone();
    let changed = !prev.is_empty() && !live_ver.is_empty() && prev != live_ver;

    let mut reapplied_count = 0usize;
    let mut failed_count = 0usize;

    if (changed || force_reapply) && !store.profiles.is_empty() {
        for profile in store.profiles.values() {
            match write_exe_registry_preferences(profile) {
                Ok(_) => reapplied_count += 1,
                Err(_) => failed_count += 1,
            }
        }
        if reapplied_count > 0 {
            store.guard_reapply_count = store.guard_reapply_count.saturating_add(1);
            store.last_reapply_epoch_sec = now_epoch_sec();
        }
    }

    store.driver_changed_last_boot = changed;
    if !live_ver.is_empty() {
        store.last_seen_driver_version = live_ver.clone();
    }
    if !gpu.adapter_name.is_empty() {
        store.gpu_adapter_name = gpu.adapter_name;
    }
    save_latency_store(&store)?;

    Ok(DriverGuardSyncReport {
        driver_changed: changed,
        previous_driver: prev,
        current_driver: live_ver,
        reapplied_count,
        failed_count,
        guard_reapply_total: store.guard_reapply_count,
    })
}

pub fn spawn_startup_driver_profile_guard(app: tauri::AppHandle) {
    use tauri::Emitter;
    std::thread::spawn(move || {
        if let Ok(report) = run_driver_profile_guard_internal(false) {
            if report.driver_changed || report.reapplied_count > 0 {
                let _ = app.emit("latency-driver-guard", &report);
            }
        }
    });
}

#[tauri::command]
pub async fn get_latency_lab_status() -> Result<LatencyLabSystemStatus, String> {
    tokio::task::spawn_blocking(|| {
        let gpu = crate::shader_swarm::detect_gpu_hardware_identity();
        let live_ver = detect_primary_gpu_driver_version().unwrap_or_else(|| gpu.driver_version.clone());
        let store = load_latency_store();
        let reg_entries = reg_enum_string_values(false, REG_USER_GPU_PREFS).len();
        let buckets = measure_shader_cache_buckets();
        let total_cache: u64 = buckets.iter().map(|b| b.size_bytes).sum();
        let mpo = probe_mpo_diagnostics();
        let gpu_vendor = match gpu.vendor_id {
            0x10DE => "NVIDIA".to_string(),
            0x1002 => "AMD".to_string(),
            0x8086 => "Intel".to_string(),
            other => format!("0x{:04X}", other),
        };

        Ok(LatencyLabSystemStatus {
            driver_version: live_ver,
            gpu_name: gpu.adapter_name,
            gpu_vendor,
            driver_changed_last_boot: store.driver_changed_last_boot,
            guard_reapply_count: store.guard_reapply_count,
            saved_profile_count: store.profiles.len(),
            registry_entry_count: reg_entries,
            shader_cache_cap_mb: store.shader_cache_cap_mb,
            shader_cache_total_bytes: total_cache,
            shader_cache_total_formatted: format_bytes(total_cache),
            shader_cache_buckets: buckets,
            mpo,
        })
    })
    .await
    .map_err(|e| format!("Latency Lab status task failed: {}", e))?
}

#[tauri::command]
pub async fn get_game_latency_profile(exe_path: String) -> Result<LatencyLabGameProfile, String> {
    tokio::task::spawn_blocking(move || Ok(read_exe_registry_preferences(&exe_path)))
        .await
        .map_err(|e| format!("Failed reading game latency profile: {}", e))?
}

#[tauri::command]
pub async fn apply_game_latency_profile(
    exe_path: String,
    gpu_preference: u8,
    flip_model_upgrade: bool,
    auto_hdr_enable: bool,
    auto_sr_mode: String,
) -> Result<LatencyLabGameProfile, String> {
    tokio::task::spawn_blocking(move || {
        let clean_exe = exe_path.trim().to_string();
        if clean_exe.is_empty() {
            return Err("Game executable path is required.".to_string());
        }
        let mut store = load_latency_store();
        let profile = LatencyLabGameProfile {
            exe_path: clean_exe.clone(),
            gpu_preference: gpu_preference.min(2),
            flip_model_upgrade,
            auto_hdr_enable,
            auto_sr_mode: normalize_auto_sr_mode(&auto_sr_mode),
            updated_at_epoch_sec: now_epoch_sec(),
        };
        write_exe_registry_preferences(&profile)?;
        store
            .profiles
            .insert(normalize_exe_key(&clean_exe), profile.clone());
        save_latency_store(&store)?;
        Ok(profile)
    })
    .await
    .map_err(|e| format!("Apply game latency profile task failed: {}", e))?
}

#[tauri::command]
pub async fn remove_game_latency_profile(exe_path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let clean = exe_path.trim();
        let mut store = load_latency_store();
        store.profiles.remove(&normalize_exe_key(clean));
        save_latency_store(&store)?;
        let _ = reg_delete_value(false, REG_USER_GPU_PREFS, clean);
        Ok("Cleared Windows DirectX UserGpuPreferences entry.".to_string())
    })
    .await
    .map_err(|e| format!("Remove latency profile task failed: {}", e))?
}

#[tauri::command]
pub async fn run_driver_profile_guard(force_reapply: Option<bool>) -> Result<DriverGuardSyncReport, String> {
    let force = force_reapply.unwrap_or(false);
    tokio::task::spawn_blocking(move || run_driver_profile_guard_internal(force))
        .await
        .map_err(|e| format!("Driver profile guard task failed: {}", e))?
}

#[tauri::command]
pub async fn enforce_shader_cache_cap(cap_mb: u64) -> Result<ShaderCacheTrimReport, String> {
    tokio::task::spawn_blocking(move || {
        let clamped_mb = cap_mb.clamp(512, 32768);
        let mut store = load_latency_store();
        store.shader_cache_cap_mb = clamped_mb;
        save_latency_store(&store)?;

        let cap_bytes = clamped_mb.saturating_mul(1024 * 1024);
        let dirs = vendor_shader_cache_dirs();
        let mut files: Vec<(PathBuf, u64, u64)> = Vec::new();
        let mut before_bytes = 0u64;

        for dir in dirs {
            if !dir.is_dir() {
                continue;
            }
            for entry in WalkDir::new(&dir).follow_links(false).into_iter().flatten() {
                if !entry.file_type().is_file() {
                    continue;
                }
                let Ok(meta) = entry.metadata() else {
                    continue;
                };
                let len = meta.len();
                before_bytes = before_bytes.saturating_add(len);
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                files.push((entry.path().to_path_buf(), len, modified));
            }
        }

        // Prune oldest shader cache blobs first only when total footprint exceeds the user's cap.
        let mut after_bytes = before_bytes;
        let mut files_pruned = 0usize;
        if before_bytes > cap_bytes {
            files.sort_by_key(|f| f.2);
            for (path, len, _) in files {
                if after_bytes <= cap_bytes {
                    break;
                }
                if fs::remove_file(&path).is_ok() {
                    after_bytes = after_bytes.saturating_sub(len);
                    files_pruned += 1;
                }
            }
        }

        let freed_bytes = before_bytes.saturating_sub(after_bytes);
        Ok(ShaderCacheTrimReport {
            cap_mb: clamped_mb,
            before_bytes,
            after_bytes,
            freed_bytes,
            freed_formatted: format_bytes(freed_bytes),
            files_pruned,
        })
    })
    .await
    .map_err(|e| format!("Shader cache cap task failed: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_gpu_preferences_formats_autosr_1080p_undocked_and_preserves_unknown_tokens() {
        let mut profile = LatencyLabGameProfile::default_for_exe(r"C:\Games\Sample\game.exe");
        profile.gpu_preference = 2;
        profile.flip_model_upgrade = true;
        profile.auto_hdr_enable = true;
        profile.auto_sr_mode = "1080p".to_string();

        let existing = "VRREnable=1;GpuPreference=0;DirectFlipDisable=0;";
        let formatted = format_user_gpu_preferences(Some(existing), &profile);

        assert!(formatted.ends_with(';'));
        assert!(formatted.contains("GpuPreference=2;"));
        assert!(formatted.contains("SwapEffectUpgradeEnable=1;"));
        assert!(formatted.contains("AutoHDREnable=1;"));
        assert!(formatted.contains("AutoSREnable=1;AutoSRProfile=1080p;AutoSRResolution=1920x1080;"));
        // Unmanaged OS/vendor tokens must be preserved across rewrites.
        assert!(formatted.contains("VRREnable=1;"));
        assert!(formatted.contains("DirectFlipDisable=0;"));

        let parsed = parse_user_gpu_preferences(&profile.exe_path, &formatted);
        assert_eq!(parsed.gpu_preference, 2);
        assert!(parsed.flip_model_upgrade);
        assert!(parsed.auto_hdr_enable);
        assert_eq!(parsed.auto_sr_mode, "1080p");

        profile.auto_sr_mode = "undocked".to_string();
        profile.auto_hdr_enable = false;
        let formatted_undocked = format_user_gpu_preferences(Some(&formatted), &profile);
        assert!(formatted_undocked.contains("AutoSRProfile=undocked;AutoSRResolution=1280x720;"));
        assert!(formatted_undocked.contains("AutoHDREnable=0;"));
        let reparsed = parse_user_gpu_preferences(&profile.exe_path, &formatted_undocked);
        assert_eq!(reparsed.auto_sr_mode, "undocked");
        assert!(!reparsed.auto_hdr_enable);
    }

    #[test]
    fn mpo_diagnostics_probe_returns_valid_telemetry() {
        let mpo = probe_mpo_diagnostics();
        assert!(!mpo.recommendation.is_empty());
        assert!(mpo.timer_resolution_ms >= 0.0);
    }
}
