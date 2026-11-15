//! "Duke Verified" Anonymous Crowd & Local Compatibility Matrix.
//!
//! Records launch outcomes (`boot_ok`, `crash_code`, `avg_fps`, `needs_dxvk`, `needs_4gb_laa`,
//! `best_emulator_core`) strictly keyed by binary **BLAKE3 hash** (never storing usernames,
//! hostnames, OS/hardware GUIDs, or personal paths before Swarm/JSON sharing).
//! Also auto-applies proven compatibility helpers on first launch and maps crash codes directly
//! to matching PCGamingWiki fix sections.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::pe_inspect::{
    deploy_or_remove_dxvk_in_dirs, inspect_compat_with_dirs, set_pe_large_address_aware,
};
use crate::snapshots::{create_snapshot_in_dir, CAS_DIR_NAME};
use crate::{atomic_write_string, get_app_data_file_standalone};

const VERIFIED_MATRIX_FILE: &str = "duke_verified_matrix.json";
const HASH_BUF_BYTES: usize = 65_536;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct VerifiedCompatEntry {
    pub exe_blake3: String,
    pub total_reports: u32,
    pub boot_ok_count: u32,
    pub crash_count: u32,
    pub last_crash_code: Option<String>,
    pub avg_fps: Option<f64>,
    pub needs_dxvk: bool,
    pub needs_4gb_laa: bool,
    pub best_emulator_core: Option<String>,
    pub pcgw_fix_section: Option<String>,
    pub pcgw_fix_url: Option<String>,
    pub recommended_actions: Vec<String>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct VerifiedMatrixDisk {
    version: u32,
    entries: BTreeMap<String, VerifiedCompatEntry>,
    auto_applied_hashes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DukeVerifiedStatusReport {
    pub game_id: String,
    pub exe_blake3: String,
    pub verified_tier: String, // "Duke Verified" | "Community Assisted" | "Needs Fixes" | "Unverified"
    pub success_rate_pct: f64,
    pub entry: Option<VerifiedCompatEntry>,
    pub auto_applied_first_launch: bool,
    pub summary: String,
}

fn verified_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn hash_binary_blake3_streaming(path: &Path) -> Result<String, String> {
    if !path.is_file() {
        return Err(format!("Binary does not exist: {}", path.display()));
    }
    let mut file =
        File::open(path).map_err(|e| format!("Failed opening {}: {e}", path.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; HASH_BUF_BYTES];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("Failed reading {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn load_matrix() -> VerifiedMatrixDisk {
    let Ok(path) = get_app_data_file_standalone(VERIFIED_MATRIX_FILE) else {
        return VerifiedMatrixDisk {
            version: 1,
            ..Default::default()
        };
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return VerifiedMatrixDisk {
            version: 1,
            ..Default::default()
        };
    };
    serde_json::from_str(&raw).unwrap_or(VerifiedMatrixDisk {
        version: 1,
        ..Default::default()
    })
}

fn save_matrix(disk: &VerifiedMatrixDisk) -> Result<(), String> {
    let path = get_app_data_file_standalone(VERIFIED_MATRIX_FILE)?;
    let json = serde_json::to_string_pretty(disk)
        .map_err(|e| format!("Failed serializing verified matrix: {e}"))?;
    atomic_write_string(&path, &json)
}

pub fn map_crash_code_to_pcgw_section(code_hex: &str) -> (String, String, Vec<String>) {
    let norm = code_hex.trim().to_ascii_uppercase();
    if norm.contains("C000007B") {
        return (
            "Visual C++ Redistributables / 32-bit vs 64-bit DLL Mismatch".to_string(),
            "https://www.pcgamingwiki.com/wiki/Glossary:Native_API#Visual_C.2B.2B".to_string(),
            vec![
                "Heal missing Visual C++ / DirectX runtime DLLs in game folder".to_string(),
                "Verify DirectX runtime & architecture match via Pre-Flight Check".to_string(),
            ],
        );
    }
    if norm.contains("C0000005") {
        return (
            "Access Violation / 2 GB Address Space Exhaustion".to_string(),
            "https://www.pcgamingwiki.com/wiki/Glossary:Large_Address_Aware".to_string(),
            vec![
                "Enable 4 GB Large Address Aware (LAA) PE flag for 32-bit titles".to_string(),
                "Try DXVK translation wrapper if DX9 allocation fails".to_string(),
            ],
        );
    }
    if norm.contains("C0000135") || norm.contains("8007007E") {
        return (
            "Missing Dependency DLL / Blocked Executable".to_string(),
            "https://www.pcgamingwiki.com/wiki/PCGamingWiki:Editing_guide/Essential_improvements".to_string(),
            vec![
                "Run 1-Click Auto-Fix (.duke-cas) to unpack runtime DLLs & unblock Mark-of-the-Web".to_string(),
            ],
        );
    }
    (
        "Crash to Desktop Workarounds".to_string(),
        "https://www.pcgamingwiki.com/wiki/Category:Games".to_string(),
        vec![
            "Check PCGamingWiki Issues Fixed & try DXVK or Windows 7/8 DPI shim".to_string(),
        ],
    )
}

fn sanitize_anonymous_entry(mut e: VerifiedCompatEntry) -> Option<VerifiedCompatEntry> {
    let hash = e.exe_blake3.trim().to_ascii_lowercase();
    if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    e.exe_blake3 = hash;
    // Strip anything that could embed paths or non-sanitized links
    if let Some(ref mut url) = e.pcgw_fix_url {
        if !url.starts_with("https://www.pcgamingwiki.com/") {
            *url = "https://www.pcgamingwiki.com/".to_string();
        }
    }
    if let Some(ref mut fps) = e.avg_fps {
        if !fps.is_finite() || *fps < 0.0 || *fps > 2000.0 {
            *fps = 60.0;
        } else {
            *fps = (*fps * 10.0).round() / 10.0;
        }
    }
    e.best_emulator_core = e
        .best_emulator_core
        .map(|s| s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').take(48).collect());
    Some(e)
}

pub fn record_verified_outcome_in_matrix(
    exe_blake3: &str,
    boot_ok: bool,
    crash_code: Option<&str>,
    avg_fps: Option<f64>,
    needs_dxvk: Option<bool>,
    needs_4gb_laa: Option<bool>,
    best_emulator_core: Option<&str>,
) -> Result<VerifiedCompatEntry, String> {
    let clean_hash = exe_blake3.trim().to_ascii_lowercase();
    if clean_hash.len() != 64 {
        return Err("Invalid BLAKE3 binary hash.".to_string());
    }
    let _g = verified_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut disk = load_matrix();
    let entry = disk
        .entries
        .entry(clean_hash.clone())
        .or_insert_with(|| VerifiedCompatEntry {
            exe_blake3: clean_hash.clone(),
            ..Default::default()
        });
    entry.total_reports = entry.total_reports.saturating_add(1);
    if boot_ok {
        entry.boot_ok_count = entry.boot_ok_count.saturating_add(1);
    } else {
        entry.crash_count = entry.crash_count.saturating_add(1);
    }
    if let Some(code) = crash_code.map(str::trim).filter(|s| !s.is_empty()) {
        entry.last_crash_code = Some(code.to_string());
        let (sec, url, recs) = map_crash_code_to_pcgw_section(code);
        entry.pcgw_fix_section = Some(sec);
        entry.pcgw_fix_url = Some(url);
        for r in recs {
            if !entry.recommended_actions.contains(&r) {
                entry.recommended_actions.push(r);
            }
        }
    }
    if let Some(fps) = avg_fps.filter(|v| v.is_finite() && *v > 1.0 && *v < 2000.0) {
        entry.avg_fps = Some(match entry.avg_fps {
            Some(prev) => ((prev * 0.6 + fps * 0.4) * 10.0).round() / 10.0,
            None => (fps * 10.0).round() / 10.0,
        });
    }
    if let Some(dxvk) = needs_dxvk {
        entry.needs_dxvk = dxvk;
    }
    if let Some(laa) = needs_4gb_laa {
        entry.needs_4gb_laa = laa;
    }
    if let Some(core) = best_emulator_core.map(str::trim).filter(|s| !s.is_empty()) {
        entry.best_emulator_core = Some(core.to_string());
    }
    entry.updated_at = now_secs();
    let out = sanitize_anonymous_entry(entry.clone()).unwrap_or_else(|| entry.clone());
    save_matrix(&disk)?;
    Ok(out)
}

/// Auto-applies proven compatibility helpers (4 GB LAA / DXVK) safely before first launch.
pub fn auto_apply_verified_wrappers_on_first_launch(
    game_id: &str,
    exe_path: &Path,
    game_dir: &Path,
    wrappers_root: &Path,
) -> Option<String> {
    let hash = hash_binary_blake3_streaming(exe_path).ok()?;
    let entry = {
        let _g = verified_lock().lock().unwrap_or_else(|e| e.into_inner());
        let mut disk = load_matrix();
        if disk.auto_applied_hashes.contains(&hash) {
            return None;
        }
        let found = disk.entries.get(&hash).cloned()?;
        if !found.needs_4gb_laa && !found.needs_dxvk {
            return None;
        }
        disk.auto_applied_hashes.push(hash.clone());
        let _ = save_matrix(&disk);
        found
    };

    let status = inspect_compat_with_dirs(game_id, exe_path, wrappers_root, None).ok()?;
    let mut applied = Vec::new();
    let cas_dir = game_dir.join(CAS_DIR_NAME);

    if entry.needs_4gb_laa && status.is_32bit && !status.large_address_aware {
        let _ = create_snapshot_in_dir(game_id, game_dir, &cas_dir, Some("Verified Auto-Fix: 4GB LAA"));
        if set_pe_large_address_aware(exe_path, true).is_ok() {
            applied.push("4 GB LAA Patch");
        }
    }
    if entry.needs_dxvk && status.dxvk_source_ready && !status.dxvk_deployed {
        let _ = create_snapshot_in_dir(game_id, game_dir, &cas_dir, Some("Verified Auto-Fix: DXVK"));
        if deploy_or_remove_dxvk_in_dirs(game_id, exe_path, wrappers_root, true, None).is_ok() {
            applied.push("DXVK Wrapper");
        }
    }

    if applied.is_empty() {
        None
    } else {
        Some(format!(
            "Duke Verified Autopilot applied: {}",
            applied.join(" + ")
        ))
    }
}

pub fn export_anonymous_verified_json() -> Result<String, String> {
    let _g = verified_lock().lock().unwrap_or_else(|e| e.into_inner());
    let disk = load_matrix();
    let clean_entries: Vec<VerifiedCompatEntry> = disk
        .entries
        .into_values()
        .filter_map(sanitize_anonymous_entry)
        .collect();
    let payload = serde_json::json!({
        "schema": "duke-verified-anon-v1",
        "exportedAt": now_secs(),
        "entries": clean_entries,
    });
    serde_json::to_string_pretty(&payload)
        .map_err(|e| format!("Failed serializing anonymous verified matrix: {e}"))
}

pub fn import_anonymous_verified_json(raw_json: &str) -> Result<usize, String> {
    let val: serde_json::Value =
        serde_json::from_str(raw_json).map_err(|e| format!("Invalid verified matrix JSON: {e}"))?;
    let list_val = val
        .get("entries")
        .ok_or_else(|| "Verified matrix JSON missing 'entries' array.".to_string())?;
    let incoming: Vec<VerifiedCompatEntry> = serde_json::from_value(list_val.clone())
        .map_err(|e| format!("Malformed verified entries: {e}"))?;
    let _g = verified_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut disk = load_matrix();
    let mut merged = 0usize;
    for item in incoming {
        if let Some(clean) = sanitize_anonymous_entry(item) {
            disk.entries
                .entry(clean.exe_blake3.clone())
                .and_modify(|cur| {
                    cur.total_reports = cur.total_reports.saturating_add(clean.total_reports);
                    cur.boot_ok_count = cur.boot_ok_count.saturating_add(clean.boot_ok_count);
                    cur.crash_count = cur.crash_count.saturating_add(clean.crash_count);
                    cur.needs_dxvk |= clean.needs_dxvk;
                    cur.needs_4gb_laa |= clean.needs_4gb_laa;
                    if cur.avg_fps.is_none() {
                        cur.avg_fps = clean.avg_fps;
                    }
                    if cur.best_emulator_core.is_none() {
                        cur.best_emulator_core = clean.best_emulator_core.clone();
                    }
                    if cur.pcgw_fix_section.is_none() {
                        cur.pcgw_fix_section = clean.pcgw_fix_section.clone();
                        cur.pcgw_fix_url = clean.pcgw_fix_url.clone();
                    }
                })
                .or_insert(clean);
            merged += 1;
        }
    }
    save_matrix(&disk)?;
    Ok(merged)
}

#[tauri::command]
pub async fn get_duke_verified_status(game_id: String) -> Result<DukeVerifiedStatusReport, String> {
    tokio::task::spawn_blocking(move || {
        let lib = crate::load_library_standalone()?;
        let entry = lib
            .into_iter()
            .find(|g| g.id == game_id)
            .ok_or_else(|| format!("Game '{}' not found.", game_id))?;
        let exe_path = PathBuf::from(&entry.exe_path);
        let hash = hash_binary_blake3_streaming(&exe_path)?;
        let _g = verified_lock().lock().unwrap_or_else(|e| e.into_inner());
        let disk = load_matrix();
        let rec = disk.entries.get(&hash).cloned();
        let auto_applied = disk.auto_applied_hashes.contains(&hash);
        let (tier, rate, summary) = if let Some(ref e) = rec {
            let total = e.total_reports.max(1) as f64;
            let pct = ((e.boot_ok_count as f64 / total) * 1000.0).round() / 10.0;
            let tier = if e.boot_ok_count > 0 && pct >= 80.0 {
                "Duke Verified"
            } else if e.needs_dxvk || e.needs_4gb_laa {
                "Community Assisted"
            } else if e.crash_count > 0 {
                "Needs Fixes"
            } else {
                "Unverified"
            };
            let fps_note = e
                .avg_fps
                .map(|f| format!(" · Avg {:.1} FPS", f))
                .unwrap_or_default();
            (
                tier.to_string(),
                pct,
                format!(
                    "{tier} · {pct:.0}% clean boot ({}/{} runs){fps_note} · BLAKE3 {}..",
                    e.boot_ok_count,
                    e.total_reports,
                    &hash[..10]
                ),
            )
        } else {
            (
                "Unverified".to_string(),
                0.0,
                format!("No local runs recorded yet · Keyed anonymously by BLAKE3 {}..", &hash[..10]),
            )
        };
        Ok(DukeVerifiedStatusReport {
            game_id,
            exe_blake3: hash,
            verified_tier: tier,
            success_rate_pct: rate,
            entry: rec,
            auto_applied_first_launch: auto_applied,
            summary,
        })
    })
    .await
    .map_err(|e| format!("Verified status task failed: {e}"))?
}

#[tauri::command]
pub async fn record_duke_verified_feedback(
    game_id: String,
    boot_ok: bool,
    crash_code: Option<String>,
    avg_fps: Option<f64>,
    needs_dxvk: Option<bool>,
    needs_4gb_laa: Option<bool>,
    best_emulator_core: Option<String>,
) -> Result<VerifiedCompatEntry, String> {
    tokio::task::spawn_blocking(move || {
        let lib = crate::load_library_standalone()?;
        let entry = lib
            .into_iter()
            .find(|g| g.id == game_id)
            .ok_or_else(|| format!("Game '{}' not found.", game_id))?;
        let hash = hash_binary_blake3_streaming(Path::new(&entry.exe_path))?;
        record_verified_outcome_in_matrix(
            &hash,
            boot_ok,
            crash_code.as_deref(),
            avg_fps,
            needs_dxvk,
            needs_4gb_laa,
            best_emulator_core.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("Record verified feedback failed: {e}"))?
}

#[tauri::command]
pub async fn export_duke_verified_matrix() -> Result<String, String> {
    tokio::task::spawn_blocking(export_anonymous_verified_json)
        .await
        .map_err(|e| format!("Export matrix failed: {e}"))?
}

#[tauri::command]
pub async fn import_duke_verified_matrix(json_payload: String) -> Result<usize, String> {
    tokio::task::spawn_blocking(move || import_anonymous_verified_json(&json_payload))
        .await
        .map_err(|e| format!("Import matrix failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anonymous_matrix_export_contains_zero_pii_or_paths() {
        let sample = VerifiedCompatEntry {
            exe_blake3: "a".repeat(64),
            total_reports: 4,
            boot_ok_count: 4,
            crash_count: 0,
            last_crash_code: Some("0xC0000005".to_string()),
            avg_fps: Some(119.44),
            needs_dxvk: true,
            needs_4gb_laa: true,
            best_emulator_core: Some("duckstation_swan".to_string()),
            pcgw_fix_section: Some("Large Address Aware".to_string()),
            pcgw_fix_url: Some("https://evil.example.com/tracker".to_string()),
            recommended_actions: vec!["Enable LAA".to_string()],
            updated_at: 1_700_000_000,
        };
        let sanitized = sanitize_anonymous_entry(sample).unwrap();
        assert_eq!(sanitized.pcgw_fix_url.as_deref(), Some("https://www.pcgamingwiki.com/"));
        assert_eq!(sanitized.avg_fps, Some(119.4));
        let json = serde_json::to_string(&sanitized).unwrap();
        assert!(!json.contains("C:\\Users"));
        assert!(!json.contains("hostname"));
        assert!(!json.contains("username"));
    }

    #[test]
    fn crash_codes_link_to_matching_pcgw_fix_sections() {
        let (sec, url, actions) = map_crash_code_to_pcgw_section("0xC000007B");
        assert!(sec.contains("Visual C++"));
        assert!(url.starts_with("https://www.pcgamingwiki.com/"));
        assert!(!actions.is_empty());
    }
}
