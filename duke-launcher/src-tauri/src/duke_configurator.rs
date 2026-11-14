use crate::duke_ai_rt::acquire_worker_lease;
use crate::shader_swarm::detect_gpu_hardware_identity;
use crate::snapshots::{create_snapshot_in_dir_filtered, CAS_DIR_NAME};
use crate::{atomic_write_string, get_app_data_file_standalone, load_library_standalone, resolve_game_root_dir};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareDisplayProfile {
    pub adapter_name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub driver_version: String,
    pub vram_mb: u64,
    pub display_width: u32,
    pub display_height: u32,
    pub display_hz: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigPatchEntry {
    pub section: String,
    pub key: String,
    pub value: String,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredConfigTarget {
    pub file_path: String,
    pub config_kind: String, // "unreal_ini" | "bethesda_prefs" | "dolphin_ini" | "cemu_xml" | "generic_ini"
    pub patches: Vec<ConfigPatchEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfiguratorPlanReport {
    pub game_id: String,
    pub game_title: String,
    pub target_hz: u32,
    pub tier_label: String,
    pub hardware: HardwareDisplayProfile,
    pub targets: Vec<DiscoveredConfigTarget>,
    pub execution_provider: String,
    pub vram_allocated_mb: f32,
    pub has_backup: bool,
    pub latest_backup_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfiguratorApplyResult {
    pub game_id: String,
    pub game_title: String,
    pub target_hz: u32,
    pub backup_id: String,
    pub cas_snapshot_id: Option<String>,
    pub files_patched: usize,
    pub patches_applied: usize,
    pub modified_paths: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigBackupRecord {
    backup_id: String,
    game_id: String,
    created_at_ms: u64,
    files: Vec<ConfigFileBackupItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigFileBackupItem {
    original_path: String,
    original_content: String,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(target_os = "windows")]
pub fn detect_hardware_display_profile() -> HardwareDisplayProfile {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplaySettingsW, DEVMODEW, ENUM_CURRENT_SETTINGS,
    };

    let base_gpu = detect_gpu_hardware_identity();
    let mut vram_mb = 8192u64;
    let mut adapter_name = base_gpu.adapter_name.clone();
    let mut vendor_id = base_gpu.vendor_id;
    let mut device_id = base_gpu.device_id;

    unsafe {
        if let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() {
            for idx in 0..8u32 {
                let Ok(adapter) = factory.EnumAdapters1(idx) else {
                    break;
                };
                if let Ok(desc) = adapter.GetDesc1() {
                    if (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
                        continue;
                    }
                    let dedicated_mb = (desc.DedicatedVideoMemory as u64) / (1024 * 1024);
                    if dedicated_mb > 0 {
                        vram_mb = dedicated_mb;
                        vendor_id = desc.VendorId;
                        device_id = desc.DeviceId;
                        let end = desc
                            .Description
                            .iter()
                            .position(|&c| c == 0)
                            .unwrap_or(desc.Description.len());
                        adapter_name = String::from_utf16_lossy(&desc.Description[..end])
                            .trim()
                            .to_string();
                        break;
                    }
                }
            }
        }
    }

    let mut dm = DEVMODEW {
        dmSize: std::mem::size_of::<DEVMODEW>() as u16,
        ..Default::default()
    };
    let (w, h, hz) = unsafe {
        if EnumDisplaySettingsW(None, ENUM_CURRENT_SETTINGS, &mut dm).as_bool() {
            (
                dm.dmPelsWidth.max(1280),
                dm.dmPelsHeight.max(720),
                dm.dmDisplayFrequency.max(60),
            )
        } else {
            (1920, 1080, 60)
        }
    };

    HardwareDisplayProfile {
        adapter_name,
        vendor_id,
        device_id,
        driver_version: base_gpu.driver_version,
        vram_mb,
        display_width: w,
        display_height: h,
        display_hz: hz,
    }
}

#[cfg(not(target_os = "windows"))]
pub fn detect_hardware_display_profile() -> HardwareDisplayProfile {
    HardwareDisplayProfile {
        adapter_name: "Software Adapter".to_string(),
        vendor_id: 0,
        device_id: 0,
        driver_version: "0.0".to_string(),
        vram_mb: 8192,
        display_width: 1920,
        display_height: 1080,
        display_hz: 60,
    }
}

fn classify_config_kind(path: &Path) -> Option<&'static str> {
    let fname = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    if fname == "engine.ini" || fname == "gameusersettings.ini" || fname == "scalability.ini" {
        Some("unreal_ini")
    } else if fname == "prefs.ini" || fname.ends_with("prefs.ini") {
        Some("bethesda_prefs")
    } else if fname == "dolphin.ini" || fname == "gfx.ini" {
        Some("dolphin_ini")
    } else if fname == "settings.xml"
        && path
            .to_string_lossy()
            .to_ascii_lowercase()
            .contains("cemu")
    {
        Some("cemu_xml")
    } else if fname == "settings.ini" || fname == "graphics.ini" || fname == "video.ini" {
        Some("generic_ini")
    } else {
        None
    }
}

fn discover_game_config_paths(game_dir: &Path, game_title: &str) -> Vec<(PathBuf, &'static str)> {
    let mut search_roots = vec![game_dir.to_path_buf()];
    let compact_title: String = game_title
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();

    if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
        let base = PathBuf::from(local_app);
        if !compact_title.is_empty() {
            search_roots.push(base.join(&compact_title).join("Saved").join("Config"));
        }
        search_roots.push(base.join("Dolphin Emulator").join("Config"));
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        let base = PathBuf::from(appdata);
        search_roots.push(base.join("Dolphin Emulator").join("Config"));
        search_roots.push(base.join("Cemu"));
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let docs = PathBuf::from(profile).join("Documents").join("My Games");
        if docs.is_dir() {
            if !compact_title.is_empty() {
                search_roots.push(docs.join(game_title));
                search_roots.push(docs.join(&compact_title));
            }
            if let Ok(entries) = fs::read_dir(&docs) {
                let needle = compact_title.to_ascii_lowercase();
                for entry in entries.flatten().take(48) {
                    let sub_name = entry
                        .file_name()
                        .to_string_lossy()
                        .chars()
                        .filter(|c| c.is_ascii_alphanumeric())
                        .collect::<String>()
                        .to_ascii_lowercase();
                    if !needle.is_empty() && (sub_name.contains(&needle) || needle.contains(&sub_name)) {
                        search_roots.push(entry.path());
                    }
                }
            }
        }
    }

    let mut found = Vec::new();
    for root in search_roots {
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(&root)
            .max_depth(3)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }
            if let Some(kind) = classify_config_kind(entry.path()) {
                if !found.iter().any(|(p, _): &(PathBuf, &str)| p == entry.path()) {
                    found.push((entry.path().to_path_buf(), kind));
                }
            }
        }
    }
    found.truncate(12);
    found
}

/// Generates GPU/VRAM & target refresh-rate tailored presets (`60`, `120`, `165+ Hz`).
pub fn synthesize_patches_for_kind(
    config_kind: &str,
    vram_mb: u64,
    target_hz: u32,
) -> (String, Vec<ConfigPatchEntry>) {
    let hz = if target_hz >= 165 {
        165
    } else if target_hz >= 120 {
        120
    } else {
        60
    };
    let vram_tier = if vram_mb >= 12000 {
        "Ultra VRAM (>=12 GB)"
    } else if vram_mb >= 6144 {
        "High VRAM (6–11 GB)"
    } else {
        "Lean VRAM (<6 GB)"
    };

    let (shadow_q, tex_pool_mb, screen_pct, dolphin_scale) = if hz >= 165 {
        let pool = if vram_mb >= 12000 { "4096" } else if vram_mb >= 6144 { "3072" } else { "2048" };
        ("1", pool, "85", "2")
    } else if hz >= 120 {
        let pool = if vram_mb >= 12000 { "4096" } else if vram_mb >= 6144 { "3072" } else { "1536" };
        ("2", pool, "92", "3")
    } else {
        let pool = if vram_mb >= 12000 { "6144" } else if vram_mb >= 6144 { "4096" } else { "2048" };
        ("3", pool, "100", "4")
    };

    let patches = match config_kind {
        "unreal_ini" => vec![
            ConfigPatchEntry {
                section: "/Script/Engine.GameUserSettings".to_string(),
                key: "FrameRateLimit".to_string(),
                value: format!("{}.000000", hz),
                rationale: format!("Match monitor {} Hz refresh ceiling", hz),
            },
            ConfigPatchEntry {
                section: "/Script/Engine.GameUserSettings".to_string(),
                key: "bUseVSync".to_string(),
                value: "False".to_string(),
                rationale: "Reduce input latency for Frame Pacing / VRR".to_string(),
            },
            ConfigPatchEntry {
                section: "/Script/Engine.RendererSettings".to_string(),
                key: "r.Streaming.PoolSize".to_string(),
                value: tex_pool_mb.to_string(),
                rationale: format!("Sized safely to {} ({} MB VRAM)", vram_tier, vram_mb),
            },
            ConfigPatchEntry {
                section: "/Script/Engine.RendererSettings".to_string(),
                key: "r.ScreenPercentage".to_string(),
                value: screen_pct.to_string(),
                rationale: format!("Target {} Hz pacing headroom", hz),
            },
            ConfigPatchEntry {
                section: "ScalabilityGroups".to_string(),
                key: "sg.ShadowQuality".to_string(),
                value: shadow_q.to_string(),
                rationale: "Balance shadow GPU cost for stable 1% lows".to_string(),
            },
        ],
        "bethesda_prefs" => vec![
            ConfigPatchEntry {
                section: "Display".to_string(),
                key: "iPresentInterval".to_string(),
                value: "0".to_string(),
                rationale: format!("Uncap from 60 Hz for {} Hz panel", hz),
            },
            ConfigPatchEntry {
                section: "HAVOK".to_string(),
                key: "fMaxTime".to_string(),
                value: format!("{:.6}", 1.0 / hz as f64),
                rationale: format!("Lock Havok physics delta to {} Hz", hz),
            },
            ConfigPatchEntry {
                section: "Display".to_string(),
                key: "iShadowMapResolution".to_string(),
                value: if hz >= 120 { "2048" } else { "4096" }.to_string(),
                rationale: "Prevent CPU/GPU draw-call bottlenecks".to_string(),
            },
        ],
        "dolphin_ini" => vec![
            ConfigPatchEntry {
                section: "Core".to_string(),
                key: "SyncOnSkipIdle".to_string(),
                value: "True".to_string(),
                rationale: "Prevent audio/frame desync on dual-core JIT".to_string(),
            },
            ConfigPatchEntry {
                section: "Settings".to_string(),
                key: "EFBScale".to_string(),
                value: dolphin_scale.to_string(),
                rationale: format!("{} scale tuned for {} MB VRAM", vram_tier, vram_mb),
            },
            ConfigPatchEntry {
                section: "Settings".to_string(),
                key: "ShaderCompilationMode".to_string(),
                value: "3".to_string(),
                rationale: "Hybrid Ubershaders eliminate shader-compile stutter".to_string(),
            },
        ],
        "cemu_xml" => vec![
            ConfigPatchEntry {
                section: "Graphic".to_string(),
                key: "api".to_string(),
                value: "1".to_string(),
                rationale: "Enable Vulkan async pipeline compilation".to_string(),
            },
            ConfigPatchEntry {
                section: "Graphic".to_string(),
                key: "AsyncCompile".to_string(),
                value: "true".to_string(),
                rationale: "Zero-hitch pipeline compilation at 120+ FPS".to_string(),
            },
            ConfigPatchEntry {
                section: "Graphic".to_string(),
                key: "VSync".to_string(),
                value: if hz <= 60 { "1" } else { "0" }.to_string(),
                rationale: format!("Sync policy for {} Hz", hz),
            },
        ],
        _ => vec![
            ConfigPatchEntry {
                section: "Video".to_string(),
                key: "MaxFPS".to_string(),
                value: hz.to_string(),
                rationale: format!("Match {} Hz target frame cap", hz),
            },
            ConfigPatchEntry {
                section: "Video".to_string(),
                key: "VSync".to_string(),
                value: "0".to_string(),
                rationale: "Minimize frame queue latency".to_string(),
            },
            ConfigPatchEntry {
                section: "Quality".to_string(),
                key: "ShadowQuality".to_string(),
                value: shadow_q.to_string(),
                rationale: format!("Tuned for {}", vram_tier),
            },
        ],
    };

    (format!("{} Hz / {}", hz, vram_tier), patches)
}

/// Idempotently patches INI file contents preserving unrelated sections, comments, and formatting.
pub fn patch_ini_contents(original: &str, patches: &[ConfigPatchEntry]) -> String {
    let mut lines: Vec<String> = original.lines().map(|l| l.to_string()).collect();

    for patch in patches {
        let target_sec = patch.section.trim();
        let mut current_sec = String::new();
        let mut sec_start: Option<usize> = None;
        let mut sec_end: Option<usize> = None;
        let mut updated = false;

        for (idx, line) in lines.iter_mut().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                let sec_name = &trimmed[1..trimmed.len() - 1];
                if sec_start.is_some() && sec_end.is_none() {
                    sec_end = Some(idx);
                }
                current_sec = sec_name.trim().to_string();
                if current_sec.eq_ignore_ascii_case(target_sec) && sec_start.is_none() {
                    sec_start = Some(idx);
                }
                continue;
            }
            if current_sec.eq_ignore_ascii_case(target_sec)
                && !trimmed.starts_with(';')
                && !trimmed.starts_with('#')
            {
                if let Some((k, _)) = trimmed.split_once('=') {
                    if k.trim().eq_ignore_ascii_case(patch.key.trim()) {
                        *line = format!("{}={}", patch.key, patch.value);
                        updated = true;
                        break;
                    }
                }
            }
        }

        if !updated {
            if let Some(start) = sec_start {
                let insert_at = sec_end.unwrap_or(lines.len()).max(start + 1);
                lines.insert(insert_at, format!("{}={}", patch.key, patch.value));
            } else {
                if !lines.is_empty() && !lines.last().map(|s| s.is_empty()).unwrap_or(true) {
                    lines.push(String::new());
                }
                lines.push(format!("[{}]", target_sec));
                lines.push(format!("{}={}", patch.key, patch.value));
            }
        }
    }

    let mut out = lines.join("\n");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn patch_cemu_xml_contents(original: &str, patches: &[ConfigPatchEntry]) -> String {
    let mut out = original.to_string();
    for patch in patches {
        let open = format!("<{}>", patch.key);
        let close = format!("</{}>", patch.key);
        if let (Some(s), Some(e)) = (out.find(&open), out.find(&close)) {
            if e >= s + open.len() {
                out.replace_range(s + open.len()..e, &patch.value);
                continue;
            }
        }
        let sec_close = format!("</{}>", patch.section);
        if let Some(pos) = out.find(&sec_close) {
            out.insert_str(
                pos,
                &format!("    <{}>{}</{}>\n    ", patch.key, patch.value, patch.key),
            );
        }
    }
    out
}

fn config_backup_file_for_game(game_id: &str) -> Result<PathBuf, String> {
    let safe_id: String = game_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect();
    get_app_data_file_standalone(&format!("config_backups_{}.json", safe_id))
}

fn load_config_backups(game_id: &str) -> Vec<ConfigBackupRecord> {
    let Ok(path) = config_backup_file_for_game(game_id) else {
        return Vec::new();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Vec<ConfigBackupRecord>>(&raw).ok())
        .unwrap_or_default()
}

fn save_config_backups(game_id: &str, records: &[ConfigBackupRecord]) -> Result<(), String> {
    let path = config_backup_file_for_game(game_id)?;
    let raw = serde_json::to_string_pretty(records)
        .map_err(|e| format!("Failed serializing config backup records: {}", e))?;
    atomic_write_string(&path, &raw)
}

#[tauri::command]
pub async fn analyze_game_config_preset(
    game_id: String,
    target_hz: Option<u32>,
) -> Result<ConfiguratorPlanReport, String> {
    tokio::task::spawn_blocking(move || -> Result<ConfiguratorPlanReport, String> {
        let lib = load_library_standalone()?;
        let entry = lib
            .iter()
            .find(|g| g.id == game_id)
            .ok_or_else(|| format!("Game '{}' not found in library.", game_id))?;

        let game_dir = if !entry.install_dir.trim().is_empty() {
            PathBuf::from(&entry.install_dir)
        } else {
            resolve_game_root_dir(Path::new(&entry.exe_path))
        };

        let hw = detect_hardware_display_profile();
        let hz = target_hz.unwrap_or(hw.display_hz).clamp(60, 360);
        let lease = acquire_worker_lease("configurator", "phi-3.5-mini-npu.onnx");

        let mut targets = Vec::new();
        let mut tier_label = String::new();
        for (path, kind) in discover_game_config_paths(&game_dir, &entry.title) {
            let (tier, patches) = synthesize_patches_for_kind(kind, hw.vram_mb, hz);
            tier_label = tier;
            targets.push(DiscoveredConfigTarget {
                file_path: path.to_string_lossy().to_string(),
                config_kind: kind.to_string(),
                patches,
            });
        }

        if targets.is_empty() {
            let (tier, patches) = synthesize_patches_for_kind("unreal_ini", hw.vram_mb, hz);
            tier_label = tier;
            let default_ini = game_dir.join("DukePreset_Engine.ini");
            targets.push(DiscoveredConfigTarget {
                file_path: default_ini.to_string_lossy().to_string(),
                config_kind: "unreal_ini".to_string(),
                patches,
            });
        }

        let backups = load_config_backups(&entry.id);
        let latest_backup_id = backups.first().map(|b| b.backup_id.clone());

        Ok(ConfiguratorPlanReport {
            game_id: entry.id.clone(),
            game_title: entry.title.clone(),
            target_hz: hz,
            tier_label,
            hardware: hw,
            targets,
            execution_provider: lease.ep_label().to_string(),
            vram_allocated_mb: lease.ep.vram_allocated_mb(),
            has_backup: latest_backup_id.is_some(),
            latest_backup_id,
        })
    })
    .await
    .map_err(|e| format!("Duke Configurator analysis task panicked: {}", e))?
}

#[tauri::command]
pub async fn apply_game_config_preset(
    game_id: String,
    target_hz: Option<u32>,
) -> Result<ConfiguratorApplyResult, String> {
    let plan = analyze_game_config_preset(game_id.clone(), target_hz).await?;
    tokio::task::spawn_blocking(move || -> Result<ConfiguratorApplyResult, String> {
        let lib = load_library_standalone()?;
        let entry = lib
            .iter()
            .find(|g| g.id == game_id)
            .ok_or_else(|| format!("Game '{}' not found.", game_id))?;
        let game_dir = if !entry.install_dir.trim().is_empty() {
            PathBuf::from(&entry.install_dir)
        } else {
            resolve_game_root_dir(Path::new(&entry.exe_path))
        };
        let cas_dir = game_dir.join(CAS_DIR_NAME);

        // Snapshot game root configs in .duke-cas/ before applying changes
        let cas_snapshot_id = create_snapshot_in_dir_filtered(
            &entry.id,
            &game_dir,
            &cas_dir,
            Some(&format!("Pre-Config {}Hz", plan.target_hz)),
            Some(&["Config", "Saved"]),
        )
        .ok()
        .map(|m| m.id);

        let mut backup_items = Vec::new();
        let mut modified_paths = Vec::new();
        let mut patches_applied = 0usize;

        for target in &plan.targets {
            let p = PathBuf::from(&target.file_path);
            let orig = fs::read_to_string(&p).unwrap_or_default();
            backup_items.push(ConfigFileBackupItem {
                original_path: target.file_path.clone(),
                original_content: orig.clone(),
            });

            let patched = if target.config_kind == "cemu_xml" {
                patch_cemu_xml_contents(&orig, &target.patches)
            } else {
                patch_ini_contents(&orig, &target.patches)
            };

            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed creating config dir '{}': {}", parent.display(), e))?;
            }
            atomic_write_string(&p, &patched)?;
            modified_paths.push(target.file_path.clone());
            patches_applied += target.patches.len();
        }

        let backup_id = format!("cfgbak_{}", now_ms());
        let mut backups = load_config_backups(&entry.id);
        backups.insert(
            0,
            ConfigBackupRecord {
                backup_id: backup_id.clone(),
                game_id: entry.id.clone(),
                created_at_ms: now_ms(),
                files: backup_items,
            },
        );
        backups.truncate(10);
        save_config_backups(&entry.id, &backups)?;

        Ok(ConfiguratorApplyResult {
            game_id: entry.id.clone(),
            game_title: entry.title.clone(),
            target_hz: plan.target_hz,
            backup_id,
            cas_snapshot_id,
            files_patched: modified_paths.len(),
            patches_applied,
            modified_paths,
            message: format!(
                "Applied {} {} Hz presets across {} config file(s) with .duke-cas backup.",
                plan.tier_label, plan.target_hz, plan.targets.len()
            ),
        })
    })
    .await
    .map_err(|e| format!("Duke Configurator apply task panicked: {}", e))?
}

#[tauri::command]
pub async fn rollback_game_config_preset(
    game_id: String,
    backup_id: Option<String>,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || -> Result<String, String> {
        let backups = load_config_backups(&game_id);
        let target = if let Some(bid) = backup_id.filter(|s| !s.trim().is_empty()) {
            backups.iter().find(|b| b.backup_id == bid)
        } else {
            backups.first()
        }
        .ok_or_else(|| "No config backup found to roll back.".to_string())?;

        let mut restored = 0usize;
        for item in &target.files {
            let p = PathBuf::from(&item.original_path);
            if item.original_content.is_empty() && p.exists() {
                let _ = fs::remove_file(&p);
            } else {
                if let Some(parent) = p.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                atomic_write_string(&p, &item.original_content)?;
            }
            restored += 1;
        }
        Ok(format!(
            "Restored {} original config file(s) from backup '{}'.",
            restored, target.backup_id
        ))
    })
    .await
    .map_err(|e| format!("Duke Configurator rollback task panicked: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_patcher_updates_existing_keys_and_preserves_comments_and_sections() {
        let src = "; Duke test config\n[Display]\niPresentInterval=1\nfDefaultFOV=90\n\n[Audio]\nMasterVolume=0.8\n";
        let patches = vec![
            ConfigPatchEntry {
                section: "Display".to_string(),
                key: "iPresentInterval".to_string(),
                value: "0".to_string(),
                rationale: "Unlocks 120Hz".to_string(),
            },
            ConfigPatchEntry {
                section: "HAVOK".to_string(),
                key: "fMaxTime".to_string(),
                value: "0.008333".to_string(),
                rationale: "120Hz delta".to_string(),
            },
        ];
        let patched = patch_ini_contents(src, &patches);
        assert!(patched.contains("; Duke test config"));
        assert!(patched.contains("iPresentInterval=0"));
        assert!(patched.contains("fDefaultFOV=90"));
        assert!(patched.contains("MasterVolume=0.8"));
        assert!(patched.contains("[HAVOK]\nfMaxTime=0.008333"));
    }

    #[test]
    fn preset_synthesizer_adapts_vram_and_refresh_rate() {
        let (_t60, p60) = synthesize_patches_for_kind("unreal_ini", 16384, 60);
        let (_t165, p165) = synthesize_patches_for_kind("unreal_ini", 4096, 165);
        assert!(p60.iter().any(|p| p.key == "FrameRateLimit" && p.value.starts_with("60.")));
        assert!(p165.iter().any(|p| p.key == "FrameRateLimit" && p.value.starts_with("165.")));
        assert!(p60.iter().any(|p| p.key == "r.Streaming.PoolSize" && p.value == "6144"));
        assert!(p165.iter().any(|p| p.key == "r.Streaming.PoolSize" && p.value == "2048"));
    }
}
