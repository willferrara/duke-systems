//! Native RetroAchievements API Integration & Pure-Rust `rc_hash` Hashing Engine.
//!
//! - Uses credentials stored strictly in the Windows DPAPI Vault (`privacy_vault.rs`).
//! - Implements `rc_hash`-compatible ROM & ISO-9660 executable hashing in pure Rust using
//!   heap-allocated 64 KB buffers (stripping 16-byte iNES headers on NES, 512-byte copier headers
//!   on SNES, and parsing PS1/PS2 `SYSTEM.CNF` boot executables or PSP `PARAM.SFO`+`EBOOT.BIN`).
//! - Emits native `achievement-unlocked` desktop popups when new RetroAchievements unlock and
//!   tracks Hardcore Mode progress badges.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::Emitter;

use crate::bios_vault::{hash_file_md5_sha1_streaming, Md5Hasher};
use crate::platforms::{
    PLATFORM_GAMECUBE, PLATFORM_NES, PLATFORM_PS1, PLATFORM_PS2, PLATFORM_PSP, PLATFORM_SNES,
};

const RA_CACHE_FILE: &str = "retroachievements_cache.json";
const MAX_CARTRIDGE_HASH_BYTES: u64 = 64 * 1024 * 1024;
const STREAM_BUF_BYTES: usize = 65_536;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct RetroAchievementItem {
    pub id: String,
    pub title: String,
    pub description: String,
    pub points: u32,
    pub unlocked: bool,
    pub hardcore_unlocked: bool,
    pub unlock_timestamp: u64,
    pub badge_name: String,
    pub true_ratio: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct RetroAchievementReport {
    pub game_id: String,
    pub rom_hash_md5: String,
    pub hash_method: String,
    pub ra_game_id: Option<u64>,
    pub ra_title: String,
    pub console_name: String,
    pub credentials_configured: bool,
    pub username: String,
    pub unlocked_count: usize,
    pub hardcore_count: usize,
    pub total_count: usize,
    pub completion_pct: f64,
    pub hardcore_pct: f64,
    pub newly_unlocked_titles: Vec<String>,
    pub achievements: Vec<RetroAchievementItem>,
    pub status_message: String,
    pub fetched_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RaCacheDisk {
    entries: BTreeMap<String, RetroAchievementReport>,
}

fn ra_cache_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn load_ra_cache() -> RaCacheDisk {
    let Ok(path) = crate::get_app_data_file_standalone(RA_CACHE_FILE) else {
        return RaCacheDisk::default();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<RaCacheDisk>(&s).ok())
        .unwrap_or_default()
}

fn save_ra_cache(cache: &RaCacheDisk) {
    if let Ok(path) = crate::get_app_data_file_standalone(RA_CACHE_FILE) {
        if let Ok(json) = serde_json::to_string_pretty(cache) {
            let _ = crate::atomic_write_string(&path, &json);
        }
    }
}

// ---------- Pure-Rust rcheevos (rc_hash) Compatible Console Hashing ----------

fn md5_file_with_offset(path: &Path, skip_bytes: u64, max_bytes: u64) -> Result<String, String> {
    let mut file =
        File::open(path).map_err(|e| format!("Failed opening ROM '{}': {e}", path.display()))?;
    let total_len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if skip_bytes < total_len {
        file.seek(SeekFrom::Start(skip_bytes))
            .map_err(|e| format!("Seek error on '{}': {e}", path.display()))?;
    }
    let mut md5 = Md5Hasher::new();
    let mut buf = vec![0u8; STREAM_BUF_BYTES];
    let mut read_total = 0u64;
    while read_total < max_bytes {
        let cap = ((max_bytes - read_total) as usize).min(buf.len());
        let n = file
            .read(&mut buf[..cap])
            .map_err(|e| format!("Read error on '{}': {e}", path.display()))?;
        if n == 0 {
            break;
        }
        md5.update(&buf[..n]);
        read_total = read_total.saturating_add(n as u64);
    }
    Ok(hex::encode(md5.finalize()))
}

#[derive(Debug, Clone, Copy)]
enum IsoSectorLayout {
    Iso2048,
    Raw2352Mode2,
}

fn detect_iso_layout(file: &mut File) -> Option<IsoSectorLayout> {
    let mut pvd = [0u8; 8];
    if file.seek(SeekFrom::Start(16 * 2048)).is_ok()
        && file.read_exact(&mut pvd).is_ok()
        && &pvd[1..6] == b"CD001"
    {
        return Some(IsoSectorLayout::Iso2048);
    }
    if file.seek(SeekFrom::Start(16 * 2352 + 24)).is_ok()
        && file.read_exact(&mut pvd).is_ok()
        && &pvd[1..6] == b"CD001"
    {
        return Some(IsoSectorLayout::Raw2352Mode2);
    }
    None
}

fn read_iso_logical_bytes(
    file: &mut File,
    layout: IsoSectorLayout,
    lba: u32,
    size: u32,
) -> Result<Vec<u8>, String> {
    let capped = size.min(24 * 1024 * 1024) as usize;
    let mut out = Vec::with_capacity(capped);
    let mut sector_buf = [0u8; 2048];
    let mut cur_lba = lba as u64;
    while out.len() < capped {
        let byte_offset = match layout {
            IsoSectorLayout::Iso2048 => cur_lba * 2048,
            IsoSectorLayout::Raw2352Mode2 => cur_lba * 2352 + 24,
        };
        file.seek(SeekFrom::Start(byte_offset))
            .map_err(|e| format!("ISO seek failed at LBA {cur_lba}: {e}"))?;
        let n = file
            .read(&mut sector_buf)
            .map_err(|e| format!("ISO read failed at LBA {cur_lba}: {e}"))?;
        if n == 0 {
            break;
        }
        let take = n.min(capped - out.len());
        out.extend_from_slice(&sector_buf[..take]);
        cur_lba += 1;
    }
    Ok(out)
}

fn find_iso_entry(
    file: &mut File,
    layout: IsoSectorLayout,
    dir_lba: u32,
    dir_len: u32,
    target_path: &str,
) -> Option<(u32, u32)> {
    let parts: Vec<&str> = target_path
        .trim_matches(&['/', '\\'][..])
        .split(&['/', '\\'][..])
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return None;
    }

    let mut cur_lba = dir_lba;
    let mut cur_len = dir_len;
    for (depth, want) in parts.iter().enumerate() {
        let want_clean = want.split(';').next().unwrap_or(want).to_ascii_uppercase();
        let dir_bytes = read_iso_logical_bytes(file, layout, cur_lba, cur_len).ok()?;
        let mut pos = 0usize;
        let mut found: Option<(u32, u32)> = None;
        while pos + 34 <= dir_bytes.len() {
            let rec_len = dir_bytes[pos] as usize;
            if rec_len == 0 {
                // Skip to next 2048-byte sector boundary
                let next_sec = ((pos / 2048) + 1) * 2048;
                if next_sec >= dir_bytes.len() {
                    break;
                }
                pos = next_sec;
                continue;
            }
            if pos + rec_len > dir_bytes.len() {
                break;
            }
            let entry = &dir_bytes[pos..pos + rec_len];
            let ext_lba = u32::from_le_bytes([entry[2], entry[3], entry[4], entry[5]]);
            let ext_size = u32::from_le_bytes([entry[10], entry[11], entry[12], entry[13]]);
            let flags = entry[25];
            let name_len = entry[32] as usize;
            if 33 + name_len <= entry.len() {
                let raw_name = &entry[33..33 + name_len];
                let name_str = String::from_utf8_lossy(raw_name);
                let clean_name = name_str.split(';').next().unwrap_or(&name_str).to_ascii_uppercase();
                let is_dir = (flags & 0x02) != 0;
                if clean_name == want_clean && (depth + 1 < parts.len() || !is_dir) {
                    found = Some((ext_lba, ext_size));
                    break;
                }
            }
            pos += rec_len;
        }
        let (next_lba, next_len) = found?;
        cur_lba = next_lba;
        cur_len = next_len;
    }
    Some((cur_lba, cur_len))
}

fn parse_system_cnf_boot_path(cnf_text: &str) -> Option<String> {
    for line in cnf_text.lines() {
        let trim = line.trim();
        let upper = trim.to_ascii_uppercase();
        if upper.starts_with("BOOT2") || upper.starts_with("BOOT") {
            if let Some((_, rhs)) = trim.split_once('=') {
                let cleaned = rhs
                    .trim()
                    .trim_start_matches("cdrom0:")
                    .trim_start_matches("cdrom:")
                    .trim_matches(&['/', '\\'][..])
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if !cleaned.is_empty() {
                    return Some(cleaned);
                }
            }
        }
    }
    None
}

fn hash_psx_or_psp_iso(rom_path: &Path, platform_id: u16) -> Option<(String, &'static str)> {
    let mut file = File::open(rom_path).ok()?;
    let layout = detect_iso_layout(&mut file)?;
    let pvd = read_iso_logical_bytes(&mut file, layout, 16, 2048).ok()?;
    if pvd.len() < 190 || &pvd[1..6] != b"CD001" {
        return None;
    }
    let root_lba = u32::from_le_bytes([pvd[158], pvd[159], pvd[160], pvd[161]]);
    let root_len = u32::from_le_bytes([pvd[166], pvd[167], pvd[168], pvd[169]]);

    if platform_id == PLATFORM_PSP {
        let (sfo_lba, sfo_len) =
            find_iso_entry(&mut file, layout, root_lba, root_len, "PSP_GAME/PARAM.SFO")?;
        let sfo_bytes = read_iso_logical_bytes(&mut file, layout, sfo_lba, sfo_len).ok()?;
        let (eboot_lba, eboot_len) =
            find_iso_entry(&mut file, layout, root_lba, root_len, "PSP_GAME/SYSDIR/EBOOT.BIN")?;
        let eboot_bytes = read_iso_logical_bytes(&mut file, layout, eboot_lba, eboot_len).ok()?;
        let mut md5 = Md5Hasher::new();
        md5.update(&sfo_bytes);
        md5.update(&eboot_bytes);
        return Some((hex::encode(md5.finalize()), "rc_hash:psp_sfo_eboot"));
    }

    let (cnf_lba, cnf_len) =
        find_iso_entry(&mut file, layout, root_lba, root_len, "SYSTEM.CNF")?;
    let cnf_bytes = read_iso_logical_bytes(&mut file, layout, cnf_lba, cnf_len).ok()?;
    let cnf_text = String::from_utf8_lossy(&cnf_bytes);
    let boot_rel = parse_system_cnf_boot_path(&cnf_text)?;
    let (exe_lba, exe_len) = find_iso_entry(&mut file, layout, root_lba, root_len, &boot_rel)?;
    let exe_bytes = read_iso_logical_bytes(&mut file, layout, exe_lba, exe_len).ok()?;
    let mut md5 = Md5Hasher::new();
    let exe_basename = boot_rel.rsplit(&['/', '\\'][..]).next().unwrap_or(&boot_rel);
    md5.update(exe_basename.as_bytes());
    md5.update(&exe_bytes);
    Some((
        hex::encode(md5.finalize()),
        if platform_id == PLATFORM_PS2 {
            "rc_hash:ps2_boot_exe"
        } else {
            "rc_hash:ps1_boot_exe"
        },
    ))
}

/// Computes an `rcheevos`-compatible MD5 hash according to console hardware rules.
pub fn compute_rcheevos_hash(rom_path: &Path, platform_id: u16) -> Result<(String, String), String> {
    if !rom_path.is_file() {
        return Err(format!("ROM file not found: {}", rom_path.display()));
    }
    let ext = rom_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let len = fs::metadata(rom_path).map(|m| m.len()).unwrap_or(0);

    // 1. NES: Strip 16-byte iNES (`NES\x1a`) header
    if platform_id == PLATFORM_NES || ext == "nes" {
        let mut f = File::open(rom_path).map_err(|e| format!("Failed reading NES ROM: {e}"))?;
        let mut hdr = [0u8; 4];
        let skip = if f.read_exact(&mut hdr).is_ok() && &hdr == b"NES\x1a" {
            16u64
        } else {
            0u64
        };
        let digest = md5_file_with_offset(rom_path, skip, MAX_CARTRIDGE_HASH_BYTES)?;
        return Ok((
            digest,
            if skip == 16 {
                "rc_hash:nes_ines_stripped".to_string()
            } else {
                "rc_hash:nes_raw".to_string()
            },
        ));
    }

    // 2. SNES: Strip 512-byte copier header when `len % 1024 == 512`
    if platform_id == PLATFORM_SNES || matches!(ext.as_str(), "sfc" | "smc") {
        let skip = if len % 1024 == 512 { 512u64 } else { 0u64 };
        let digest = md5_file_with_offset(rom_path, skip, MAX_CARTRIDGE_HASH_BYTES)?;
        return Ok((
            digest,
            if skip == 512 {
                "rc_hash:snes_header_stripped".to_string()
            } else {
                "rc_hash:snes_raw".to_string()
            },
        ));
    }

    // 3. Disc Consoles (PS1, PS2, PSP): parse ISO-9660 internal boot executables
    if matches!(platform_id, PLATFORM_PS1 | PLATFORM_PS2 | PLATFORM_PSP)
        || matches!(ext.as_str(), "iso" | "bin")
    {
        let eff_plat = if platform_id != 0 {
            platform_id
        } else {
            PLATFORM_PS1
        };
        if let Some((digest, method)) = hash_psx_or_psp_iso(rom_path, eff_plat) {
            return Ok((digest, method.to_string()));
        }
    }

    // 4. GameCube ISO: hash primary disc ID + apploader header region
    if platform_id == PLATFORM_GAMECUBE && ext == "iso" {
        let digest = md5_file_with_offset(rom_path, 0, (4 * 1024 * 1024).min(len))?;
        return Ok((digest, "rc_hash:gc_disc_prefix".to_string()));
    }

    // 5. Fallback for cartridge ROMs (GBA/GB/N64/NDS/Genesis)
    let (md5, _, _) = hash_file_md5_sha1_streaming(rom_path)?;
    Ok((md5, "rc_hash:md5_stream".to_string()))
}

fn resolve_ra_hash_for_game(entry: &crate::GameEntry) -> Option<(PathBuf, String, String)> {
    let exe = PathBuf::from(&entry.exe_path);
    if !exe.is_file() {
        return None;
    }
    // If `.m3u` playlist, resolve first valid disc entry
    let ext = exe
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let target_rom = if ext == "m3u" {
        let raw = fs::read_to_string(&exe).ok()?;
        let parent = exe.parent()?;
        let first_line = raw
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with('#'))?;
        let p = PathBuf::from(first_line);
        if p.is_absolute() {
            p
        } else {
            parent.join(p)
        }
    } else {
        exe
    };
    let (md5, method) = compute_rcheevos_hash(&target_rom, entry.platform_id).ok()?;
    Some((target_rom, md5, method))
}

#[tauri::command]
pub async fn fetch_retro_achievements_progress(
    app: tauri::AppHandle,
    game_id: String,
    force_refresh: Option<bool>,
) -> Result<RetroAchievementReport, String> {
    let lib = crate::load_library_standalone()?;
    let entry = lib
        .into_iter()
        .find(|g| g.id == game_id)
        .ok_or_else(|| format!("Game '{}' not found.", game_id))?;

    let hash_res = tokio::task::spawn_blocking({
        let entry_c = entry.clone();
        move || resolve_ra_hash_for_game(&entry_c)
    })
    .await
    .map_err(|e| format!("rcheevos hash failed: {e}"))?;

    let (_rom_path, rom_hash_md5, hash_method) = hash_res.unwrap_or_else(|| {
        (PathBuf::new(), String::new(), "unavailable".to_string())
    });

    let creds = crate::privacy_vault::get_retro_achievements_credentials();
    let cache_key = if !rom_hash_md5.is_empty() {
        format!("hash:{rom_hash_md5}")
    } else {
        format!("game:{}", entry.id)
    };
    let force = force_refresh.unwrap_or(false);

    if !force {
        let _g = ra_cache_lock().lock().unwrap_or_else(|e| e.into_inner());
        let cache = load_ra_cache();
        if let Some(hit) = cache.entries.get(&cache_key) {
            if now_unix_secs().saturating_sub(hit.fetched_at) < 300 {
                let mut out = hit.clone();
                out.credentials_configured = creds.is_some();
                return Ok(out);
            }
        }
    }

    let Some((username, api_key)) = creds else {
        return Ok(RetroAchievementReport {
            game_id: entry.id,
            rom_hash_md5,
            hash_method,
            credentials_configured: false,
            status_message:
                "Add your RetroAchievements username & read-only Web API key in Settings -> Privacy Vault to sync live achievements & Hardcore badges."
                    .to_string(),
            ..Default::default()
        });
    };

    if rom_hash_md5.is_empty() {
        return Ok(RetroAchievementReport {
            game_id: entry.id,
            credentials_configured: true,
            username,
            hash_method,
            status_message: "Link a valid console ROM / ISO file to look up RetroAchievements."
                .to_string(),
            ..Default::default()
        });
    }

    let client = duke_core::net_doh::get_configured_http_client();
    let lookup_url = format!(
        "https://retroachievements.org/API/API_GetGameIDFromHash.php?h={}&y={}",
        urlencoding::encode(&rom_hash_md5),
        urlencoding::encode(&api_key)
    );
    let mut ra_game_id: Option<u64> = None;
    if let Ok(resp) = client
        .get(&lookup_url)
        .timeout(Duration::from_secs(7))
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(val) = resp.json::<Value>().await {
                let id_num = val
                    .get("GameID")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                if id_num > 0 {
                    ra_game_id = Some(id_num);
                }
            }
        }
    }

    let Some(gid) = ra_game_id else {
        let rep = RetroAchievementReport {
            game_id: entry.id,
            rom_hash_md5,
            hash_method,
            credentials_configured: true,
            username,
            status_message: "ROM hash calculated cleanly; no matching RetroAchievements set found for this hash revision."
                .to_string(),
            fetched_at: now_unix_secs(),
            ..Default::default()
        };
        return Ok(rep);
    };

    let progress_url = format!(
        "https://retroachievements.org/API/API_GetGameInfoAndUserProgress.php?g={gid}&u={}&y={}",
        urlencoding::encode(&username),
        urlencoding::encode(&api_key)
    );
    let resp = client
        .get(&progress_url)
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .map_err(|e| format!("RetroAchievements API request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("RetroAchievements returned HTTP {}", resp.status()));
    }
    let val = resp
        .json::<Value>()
        .await
        .map_err(|e| format!("Invalid RetroAchievements JSON response: {e}"))?;

    let ra_title = val
        .get("Title")
        .and_then(|v| v.as_str())
        .unwrap_or(&entry.title)
        .to_string();
    let console_name = val
        .get("ConsoleName")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut items = Vec::new();
    let mut unlocked_count = 0usize;
    let mut hardcore_count = 0usize;
    if let Some(map) = val.get("Achievements").and_then(|v| v.as_object()) {
        for (id_key, a) in map {
            let unlocked = a
                .get("DateEarned")
                .and_then(|v| v.as_str())
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
            let hardcore = a
                .get("DateEarnedHardcore")
                .and_then(|v| v.as_str())
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
            if unlocked {
                unlocked_count += 1;
            }
            if hardcore {
                hardcore_count += 1;
            }
            items.push(RetroAchievementItem {
                id: id_key.clone(),
                title: a.get("Title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                description: a
                    .get("Description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                points: a.get("Points").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                unlocked,
                hardcore_unlocked: hardcore,
                unlock_timestamp: 0,
                badge_name: a
                    .get("BadgeName")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                true_ratio: a.get("TrueRatio").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            });
        }
    }
    items.sort_by(|a, b| b.unlocked.cmp(&a.unlocked).then_with(|| a.title.cmp(&b.title)));
    let total_count = items.len();
    let completion_pct = if total_count > 0 {
        (unlocked_count as f64 / total_count as f64) * 100.0
    } else {
        0.0
    };
    let hardcore_pct = if total_count > 0 {
        (hardcore_count as f64 / total_count as f64) * 100.0
    } else {
        0.0
    };

    let mut newly_unlocked_titles = Vec::new();
    {
        let _g = ra_cache_lock().lock().unwrap_or_else(|e| e.into_inner());
        let mut cache = load_ra_cache();
        if let Some(prev) = cache.entries.get(&cache_key) {
            for item in &items {
                if item.unlocked
                    && !prev
                        .achievements
                        .iter()
                        .any(|p| p.id == item.id && p.unlocked)
                {
                    newly_unlocked_titles.push(item.title.clone());
                    let _ = app.emit(
                        "achievement-unlocked",
                        serde_json::json!({
                            "gameId": entry.id,
                            "title": item.title,
                            "description": item.description,
                            "hardcore": item.hardcore_unlocked,
                            "points": item.points,
                            "source": "RetroAchievements"
                        }),
                    );
                }
            }
        }
        let report = RetroAchievementReport {
            game_id: entry.id.clone(),
            rom_hash_md5: rom_hash_md5.clone(),
            hash_method: hash_method.clone(),
            ra_game_id: Some(gid),
            ra_title: ra_title.clone(),
            console_name: console_name.clone(),
            credentials_configured: true,
            username: username.clone(),
            unlocked_count,
            hardcore_count,
            total_count,
            completion_pct,
            hardcore_pct,
            newly_unlocked_titles: newly_unlocked_titles.clone(),
            achievements: items,
            status_message: format!(
                "RetroAchievements synced: {unlocked_count}/{total_count} unlocked ({hardcore_count} Hardcore)."
            ),
            fetched_at: now_unix_secs(),
        };
        cache.entries.insert(cache_key, report.clone());
        save_ra_cache(&cache);
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bios_vault::md5_hex_bytes;
    use std::io::Write;

    #[test]
    fn nes_and_snes_rc_hash_strip_headers_accurately() {
        let dir = std::env::temp_dir().join(format!("duke_ra_hash_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let nes_payload = vec![0xAAu8; 32 * 1024];
        let mut ines = Vec::with_capacity(16 + nes_payload.len());
        ines.extend_from_slice(b"NES\x1a\x02\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00");
        ines.extend_from_slice(&nes_payload);
        let nes_path = dir.join("demo.nes");
        fs::write(&nes_path, &ines).unwrap();

        let (nes_hash, method) = compute_rcheevos_hash(&nes_path, PLATFORM_NES).unwrap();
        assert_eq!(method, "rc_hash:nes_ines_stripped");
        assert_eq!(nes_hash, md5_hex_bytes(&nes_payload));

        let snes_payload = vec![0x55u8; 64 * 1024];
        let mut smc = vec![0x01u8; 512];
        smc.extend_from_slice(&snes_payload);
        let snes_path = dir.join("demo.smc");
        fs::write(&snes_path, &smc).unwrap();
        let (snes_hash, snes_method) = compute_rcheevos_hash(&snes_path, PLATFORM_SNES).unwrap();
        assert_eq!(snes_method, "rc_hash:snes_header_stripped");
        assert_eq!(snes_hash, md5_hex_bytes(&snes_payload));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn iso9660_ps1_rc_hash_reads_system_cnf_and_boot_exe() {
        let dir = std::env::temp_dir().join(format!("duke_ra_iso_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let iso_path = dir.join("psx_demo.iso");
        let mut img = vec![0u8; 25 * 2048];
        // Primary Volume Descriptor at sector 16
        let pvd = 16 * 2048;
        img[pvd] = 1;
        img[pvd + 1..pvd + 6].copy_from_slice(b"CD001");
        img[pvd + 158..pvd + 162].copy_from_slice(&18u32.to_le_bytes()); // root dir LBA 18
        img[pvd + 166..pvd + 170].copy_from_slice(&2048u32.to_le_bytes());

        // Directory records at LBA 18
        let mut dpos = 18 * 2048;
        let write_rec = |buf: &mut [u8], pos: &mut usize, lba: u32, size: u32, name: &[u8]| {
            let rec_len = 33 + name.len();
            buf[*pos] = rec_len as u8;
            buf[*pos + 2..*pos + 6].copy_from_slice(&lba.to_le_bytes());
            buf[*pos + 10..*pos + 14].copy_from_slice(&size.to_le_bytes());
            buf[*pos + 32] = name.len() as u8;
            buf[*pos + 33..*pos + 33 + name.len()].copy_from_slice(name);
            *pos += rec_len;
        };
        let cnf = b"BOOT = cdrom:\\SLUS_001.23;1\nTCB = 4\n";
        let exe = b"PS-X EXE TEST BINARY";
        write_rec(&mut img, &mut dpos, 19, cnf.len() as u32, b"SYSTEM.CNF;1");
        write_rec(&mut img, &mut dpos, 20, exe.len() as u32, b"SLUS_001.23;1");
        img[19 * 2048..19 * 2048 + cnf.len()].copy_from_slice(cnf);
        img[20 * 2048..20 * 2048 + exe.len()].copy_from_slice(exe);

        let mut f = File::create(&iso_path).unwrap();
        f.write_all(&img).unwrap();

        let (digest, method) = compute_rcheevos_hash(&iso_path, PLATFORM_PS1).unwrap();
        assert_eq!(method, "rc_hash:ps1_boot_exe");
        let mut expected = Vec::new();
        expected.extend_from_slice(b"SLUS_001.23");
        expected.extend_from_slice(exe);
        assert_eq!(digest, md5_hex_bytes(&expected));

        let _ = fs::remove_dir_all(&dir);
    }
}
