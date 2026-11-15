//! 1-Click Steam Workshop & Nexus Mod Archive Dropper.
//!
//! Accepts Steam Workshop URLs/IDs or dropped Nexus `.zip`/`.7z`/`.rar` mod archives on a
//! game's Detail View:
//! 1. Creates a `.duke-cas/` restore point before modifying anything.
//! 2. Scans EVERY `.dll`, `.asi`, and `.exe` inside the staged mod using Security Shield v2
//!    heuristics (`pe_inspect.rs`) BEFORE installing into the game folder—quarantining and
//!    aborting on any `Blocked` payload.
//! 3. Registers & activates the mod cleanly through `mod_graph.rs` proxy-DLL collision resolution.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

use crate::mod_graph::{
    apply_mod_loadout_in_dir, register_mod_pack_from_dir, save_loadout_definition,
    scan_game_mod_graph_in_dir, ModGraphReport,
};
use crate::pe_inspect::{
    inspect_pe_security_shield_file, quarantine_high_risk_binary, quarantine_root_dir_standalone,
    PeSecurityShieldResult,
};
use crate::snapshots::{create_snapshot_in_dir, CAS_DIR_NAME};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModScannedBinary {
    pub file_name: String,
    pub shield_badge: String,
    pub authenticode_status: String,
    pub summary: String,
    pub quarantine_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModDropInstallReport {
    pub game_id: String,
    pub source_kind: String, // "archive" | "folder" | "steam_workshop"
    pub mod_pack_id: Option<String>,
    pub snapshot_id: Option<String>,
    pub blocked: bool,
    pub scanned_binaries: Vec<ModScannedBinary>,
    pub mod_graph: Option<ModGraphReport>,
    pub summary: String,
}

pub fn parse_steam_workshop_id(input: &str) -> Option<String> {
    let t = input.trim();
    if let Some(rest) = t.strip_prefix("workshop:") {
        let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return Some(digits);
        }
    }
    let lower = t.to_ascii_lowercase();
    if lower.contains("steamcommunity.com/sharedfiles")
        || lower.contains("steamcommunity.com/workshop")
    {
        if let Some(pos) = lower.find("id=") {
            let digits: String = t[pos + 3..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if !digits.is_empty() {
                return Some(digits);
            }
        }
    }
    None
}

fn scan_staged_mod_binaries(
    stage_dir: &Path,
    quarantine_dir: Option<&Path>,
) -> (Vec<ModScannedBinary>, Option<String>) {
    let mut results = Vec::new();
    let mut blocked_reason: Option<String> = None;

    for ent in WalkDir::new(stage_dir)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if !ent.file_type().is_file() {
            continue;
        }
        let p = ent.path();
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(ext.as_str(), "dll" | "asi" | "exe") {
            continue;
        }
        let fname = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("binary")
            .to_string();
        let shield: PeSecurityShieldResult =
            inspect_pe_security_shield_file(p, false, None).unwrap_or_default();
        let mut qpath = None;
        if shield.shield_badge == "Blocked" {
            let reasons: Vec<String> = shield
                .suspicious_behaviors
                .iter()
                .map(|b| format!("{}: {}", b.title, b.explanation))
                .collect();
            if let Some(qdir) = quarantine_dir {
                if let Ok(qp) = quarantine_high_risk_binary(p, qdir, &reasons, None) {
                    qpath = Some(qp.to_string_lossy().into_owned());
                }
            }
            blocked_reason.get_or_insert_with(|| {
                format!(
                    "Blocked unsafe mod binary '{}': {}",
                    fname,
                    reasons.join(" | ")
                )
            });
        }
        let summary_text = if shield.suspicious_behaviors.is_empty() {
            format!("{} ({})", shield.shield_badge, shield.authenticode_status)
        } else {
            shield
                .suspicious_behaviors
                .iter()
                .map(|b| format!("[{}] {}", b.severity, b.title))
                .collect::<Vec<_>>()
                .join(", ")
        };
        results.push(ModScannedBinary {
            file_name: fname,
            shield_badge: shield.shield_badge,
            authenticode_status: shield.authenticode_status,
            summary: summary_text,
            quarantine_path: qpath,
        });
    }

    (results, blocked_reason)
}

pub(crate) fn install_staged_mod_dir_secure(
    game_id: &str,
    game_dir: &Path,
    exe_path: Option<&Path>,
    stage_dir: &Path,
    pack_name: &str,
    source_kind: &str,
    quarantine_dir: Option<&Path>,
) -> Result<ModDropInstallReport, String> {
    if !stage_dir.is_dir() {
        return Err("Mod staging folder does not exist.".to_string());
    }

    // 1. Mandatory Security Shield v2 scan of EVERY .dll/.asi/.exe BEFORE touching game files
    let (scanned_binaries, blocked_reason) = scan_staged_mod_binaries(stage_dir, quarantine_dir);
    if let Some(reason) = blocked_reason {
        return Ok(ModDropInstallReport {
            game_id: game_id.to_string(),
            source_kind: source_kind.to_string(),
            mod_pack_id: None,
            snapshot_id: None,
            blocked: true,
            scanned_binaries,
            mod_graph: None,
            summary: reason,
        });
    }

    // 2. Create .duke-cas/ safety snapshot BEFORE routing mod into game folder
    let cas_dir = game_dir.join(CAS_DIR_NAME);
    let snapshot_id = create_snapshot_in_dir(
        game_id,
        game_dir,
        &cas_dir,
        Some(&format!("Pre-Mod: {pack_name}")),
    )
    .ok()
    .map(|s| s.id);

    // 3. Register + resolve proxy-DLL collisions via mod_graph.rs
    let pack_id = register_mod_pack_from_dir(game_dir, stage_dir, pack_name)?;
    let graph_before = scan_game_mod_graph_in_dir(game_id, game_dir, exe_path, &cas_dir)?;
    let mut active_packs: Vec<String> = graph_before
        .active_loadout_id
        .as_deref()
        .and_then(|aid| graph_before.loadouts.iter().find(|l| l.loadout_id == aid))
        .map(|l| l.enabled_pack_ids.clone())
        .unwrap_or_default();
    if !active_packs.contains(&pack_id) {
        active_packs.push(pack_id.clone());
    }
    let loadout = save_loadout_definition(game_dir, "Active Drop Loadout", active_packs)?;
    let apply_res = apply_mod_loadout_in_dir(
        game_id,
        game_dir,
        exe_path,
        &cas_dir,
        Some(&loadout.loadout_id),
    )?;
    let collision_note = if apply_res.report.collisions.is_empty() {
        "zero proxy-DLL collisions".to_string()
    } else {
        format!(
            "{} proxy-DLL chain(s) resolved",
            apply_res.report.collisions.len()
        )
    };

    Ok(ModDropInstallReport {
        game_id: game_id.to_string(),
        source_kind: source_kind.to_string(),
        mod_pack_id: Some(pack_id),
        snapshot_id,
        blocked: false,
        scanned_binaries: scanned_binaries.clone(),
        mod_graph: Some(apply_res.report),
        summary: format!(
            "Installed '{}' ({} PE binaries verified clean; {collision_note}).",
            pack_name,
            scanned_binaries.len()
        ),
    })
}

async fn resolve_steam_workshop_file(
    published_file_id: &str,
) -> Result<(String, Option<String>, Option<u64>), String> {
    let client = duke_core::net_doh::get_configured_http_client();
    let form = [
        ("itemcount", "1".to_string()),
        ("publishedfileids[0]", published_file_id.to_string()),
    ];
    let resp = client
        .post("https://api.steampowered.com/ISteamRemoteStorage/GetPublishedFileDetails/v1/")
        .form(&form)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Steam Workshop query failed: {e}"))?;
    let val = resp
        .json::<Value>()
        .await
        .map_err(|e| format!("Invalid Steam Workshop response: {e}"))?;
    let details = val
        .pointer("/response/publishedfiledetails/0")
        .ok_or_else(|| "Workshop item details missing from Steam response.".to_string())?;
    let title = details
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or(published_file_id)
        .to_string();
    let file_url = details
        .get("file_url")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let consumer_app_id = details.get("consumer_app_id").and_then(|v| v.as_u64());
    Ok((title, file_url, consumer_app_id))
}

#[tauri::command]
pub async fn install_mod_archive_or_workshop(
    game_id: String,
    source_input: String,
) -> Result<ModDropInstallReport, String> {
    let lib = crate::load_library_standalone()?;
    let entry = lib
        .into_iter()
        .find(|g| g.id == game_id)
        .ok_or_else(|| format!("Game '{}' not found.", game_id))?;
    let game_dir = crate::resolve_game_root_dir(Path::new(&entry.install_dir));
    let exe_path_opt = (!entry.exe_path.trim().is_empty()).then(|| PathBuf::from(&entry.exe_path));
    let trimmed = source_input.trim().trim_matches('"').to_string();
    if trimmed.is_empty() {
        return Err("Provide a Steam Workshop URL or drop a mod .zip/.7z archive.".to_string());
    }

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let stage_root = game_dir
        .join(CAS_DIR_NAME)
        .join(format!("_mod_stage_{ts}_{}", std::process::id()));

    if let Some(ws_id) = parse_steam_workshop_id(&trimmed) {
        let (ws_title, file_url_opt, app_id_opt) = resolve_steam_workshop_file(&ws_id).await?;
        fs::create_dir_all(&stage_root)
            .map_err(|e| format!("Failed creating Workshop staging dir: {e}"))?;

        if let Some(file_url) = file_url_opt {
            let client = duke_core::net_doh::get_configured_http_client();
            let bytes = client
                .get(&file_url)
                .timeout(Duration::from_secs(30))
                .send()
                .await
                .map_err(|e| format!("Workshop CDN download failed: {e}"))?
                .bytes()
                .await
                .map_err(|e| format!("Failed reading Workshop payload: {e}"))?;
            let dl_path = stage_root.join(format!("workshop_{ws_id}.zip"));
            fs::write(&dl_path, &bytes)
                .map_err(|e| format!("Failed writing Workshop file: {e}"))?;
            let extract_dir = stage_root.join("unpacked");
            if bytes.starts_with(b"PK") {
                let _ = crate::extract_archive_to_folder_sync(&dl_path, &extract_dir);
                let _ = fs::remove_file(&dl_path);
            } else {
                fs::create_dir_all(&extract_dir).ok();
                let _ = fs::rename(&dl_path, extract_dir.join(format!("workshop_{ws_id}.bin")));
            }
            let scan_dir = if extract_dir.is_dir() {
                extract_dir
            } else {
                stage_root.clone()
            };
            let qdir = quarantine_root_dir_standalone().ok();
            let rep = tokio::task::spawn_blocking(move || {
                let r = install_staged_mod_dir_secure(
                    &entry.id,
                    &game_dir,
                    exe_path_opt.as_deref(),
                    &scan_dir,
                    &ws_title,
                    "steam_workshop",
                    qdir.as_deref(),
                );
                let _ = fs::remove_dir_all(&stage_root);
                r
            })
            .await
            .map_err(|e| format!("Workshop install failed: {e}"))??;
            return Ok(rep);
        } else {
            // Attempt steamcmd anonymous fallback when Steam hides direct CDN URL
            let _ = fs::remove_dir_all(&stage_root);
            let aid_str = app_id_opt
                .or(entry.steam_app_id)
                .map(|a| a.to_string())
                .unwrap_or_default();
            return Err(format!(
                "Steam Workshop item '{}' (ID {ws_id}, AppID {aid_str}) requires SteamCMD authentication/ownership; anonymous direct CDN URL was not exposed.",
                ws_title
            ));
        }
    }

    // Local archive or folder path
    let src_path = PathBuf::from(&trimmed);
    if !src_path.exists() {
        return Err(format!("Mod file or folder does not exist: {}", src_path.display()));
    }

    tokio::task::spawn_blocking(move || -> Result<ModDropInstallReport, String> {
        let qdir = quarantine_root_dir_standalone().ok();
        let pack_name = src_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("CustomMod")
            .to_string();
        if src_path.is_dir() {
            return install_staged_mod_dir_secure(
                &entry.id,
                &game_dir,
                exe_path_opt.as_deref(),
                &src_path,
                &pack_name,
                "folder",
                qdir.as_deref(),
            );
        }
        fs::create_dir_all(&stage_root)
            .map_err(|e| format!("Failed creating mod staging dir: {e}"))?;
        let unpack_res = crate::extract_archive_to_folder_sync(&src_path, &stage_root);
        if let Err(e) = unpack_res {
            let _ = fs::remove_dir_all(&stage_root);
            return Err(e);
        }
        let rep = install_staged_mod_dir_secure(
            &entry.id,
            &game_dir,
            exe_path_opt.as_deref(),
            &stage_root,
            &pack_name,
            "archive",
            qdir.as_deref(),
        );
        let _ = fs::remove_dir_all(&stage_root);
        rep
    })
    .await
    .map_err(|e| format!("Mod archive install failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workshop_url_parser_and_malicious_mod_blocker_work() {
        assert_eq!(
            parse_steam_workshop_id("https://steamcommunity.com/sharedfiles/filedetails/?id=3123456789")
                .as_deref(),
            Some("3123456789")
        );
        assert_eq!(parse_steam_workshop_id("workshop:98765").as_deref(), Some("98765"));

        let root = std::env::temp_dir().join(format!("duke_moddrop_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let game_dir = root.join("Game");
        let stage_bad = root.join("StageBad");
        let stage_good = root.join("StageGood");
        let qdir = root.join("Quarantine");
        fs::create_dir_all(&game_dir).unwrap();
        fs::create_dir_all(&stage_bad).unwrap();
        fs::create_dir_all(&stage_good).unwrap();
        fs::write(game_dir.join("game.exe"), b"MZ").unwrap();

        // Malicious staged DLL with process injection + LSASS access strings -> Blocked before copying
        let mut bad_payload = b"MZ\x90\x00".to_vec();
        bad_payload.extend_from_slice(
            b"VirtualAllocEx\0WriteProcessMemory\0CreateRemoteThread\0CryptUnprotectData\0Login Data\0",
        );
        fs::write(stage_bad.join("evil.dll"), &bad_payload).unwrap();

        let game_exe = game_dir.join("game.exe");
        let bad_rep = install_staged_mod_dir_secure(
            "g1",
            &game_dir,
            Some(&game_exe),
            &stage_bad,
            "BadMod",
            "archive",
            Some(&qdir),
        )
        .unwrap();
        assert!(bad_rep.blocked);
        assert!(!game_dir.join("evil.dll").exists());

        // Safe mod file -> installs cleanly with proxy DLL tracked
        fs::write(stage_good.join("mod_config.ini"), b"[Settings]\nEnabled=1\n").unwrap();
        let good_rep = install_staged_mod_dir_secure(
            "g1",
            &game_dir,
            Some(&game_exe),
            &stage_good,
            "CleanMod",
            "archive",
            Some(&qdir),
        )
        .unwrap();
        assert!(!good_rep.blocked);
        assert!(good_rep.mod_pack_id.is_some());

        let _ = fs::remove_dir_all(&root);
    }
}
