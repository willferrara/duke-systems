use reqwest::header::{ACCEPT, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SteamGridDbGameInfo {
    pub id: u64,
    pub name: String,
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SteamGridDbAsset {
    pub id: u64,
    pub url: String,
    pub thumb: String,
    pub width: u32,
    pub height: u32,
    pub mime: String,
    pub style: String,
    pub author: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SteamGridDbSearchResult {
    pub has_api_key: bool,
    pub matched_game: Option<SteamGridDbGameInfo>,
    pub grids: Vec<SteamGridDbAsset>,
    pub heroes: Vec<SteamGridDbAsset>,
    pub icons: Vec<SteamGridDbAsset>,
    pub message: Option<String>,
}

async fn sgdb_get_json(url: &str, api_key: &str) -> Result<Value, String> {
    let client = duke_core::net_doh::build_doh_client_with_timeouts(
        url,
        Duration::from_secs(10),
        Duration::from_secs(15),
        true,
    )
    .await;

    let resp = client
        .get(url)
        .header(ACCEPT, "application/json")
        .header(AUTHORIZATION, format!("Bearer {}", api_key.trim()))
        .send()
        .await
        .map_err(|e| format!("SteamGridDB request failed: {}", e))?;

    if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 {
        return Err("SteamGridDB API key was rejected (HTTP 401/403).".to_string());
    }
    if !resp.status().is_success() {
        return Err(format!(
            "SteamGridDB API returned HTTP {} for {}",
            resp.status().as_u16(),
            url
        ));
    }

    resp.json::<Value>()
        .await
        .map_err(|e| format!("Invalid JSON from SteamGridDB: {}", e))
}

fn parse_sgdb_assets(val: &Value, limit: usize) -> Vec<SteamGridDbAsset> {
    let Some(arr) = val.get("data").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|item| {
            let url = item.get("url").and_then(|v| v.as_str())?.trim().to_string();
            if url.is_empty() {
                return None;
            }
            let thumb = item
                .get("thumb")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| url.clone());
            Some(SteamGridDbAsset {
                id: item.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
                url,
                thumb,
                width: item.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                height: item.get("height").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                mime: item
                    .get("mime")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                style: item
                    .get("style")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                author: item
                    .get("author")
                    .and_then(|a| a.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .take(limit)
        .collect()
}

/// Wraps raw PNG bytes inside a Windows Vista+ PNG-backed `.ico` container.
pub fn wrap_png_bytes_in_ico(png_bytes: &[u8]) -> Option<Vec<u8>> {
    // Verify PNG signature `\x89PNG\r\n\x1a\n`
    if png_bytes.len() < 24 || &png_bytes[0..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let size_u32 = u32::try_from(png_bytes.len()).ok()?;
    let mut ico = Vec::with_capacity(22 + png_bytes.len());
    // ICONDIR (6 bytes)
    ico.extend_from_slice(&0u16.to_le_bytes()); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // type = 1 (ICO)
    ico.extend_from_slice(&1u16.to_le_bytes()); // image count = 1
    // ICONDIRENTRY (16 bytes): width=0(256), height=0(256), colorCount=0, reserved=0, planes=1, bpp=32
    ico.push(0);
    ico.push(0);
    ico.push(0);
    ico.push(0);
    ico.extend_from_slice(&1u16.to_le_bytes());
    ico.extend_from_slice(&32u16.to_le_bytes());
    ico.extend_from_slice(&size_u32.to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes()); // image data offset
    ico.extend_from_slice(png_bytes);
    Some(ico)
}

#[tauri::command]
pub(crate) async fn search_steamgriddb_artwork(
    query_or_appid: String,
) -> Result<SteamGridDbSearchResult, String> {
    let trimmed = query_or_appid.trim().to_string();
    let settings = tokio::task::spawn_blocking(crate::load_settings_standalone)
        .await
        .map_err(|e| format!("Failed reading settings: {}", e))?;

    let api_key = settings
        .steamgriddb_api_key
        .unwrap_or_default()
        .trim()
        .to_string();
    if api_key.is_empty() {
        return Ok(SteamGridDbSearchResult {
            has_api_key: false,
            message: Some(
                "Add your free SteamGridDB API key in Settings to browse custom 600x900 grids, heroes, and .ico icons.".to_string(),
            ),
            ..Default::default()
        });
    }

    if trimmed.is_empty() {
        return Err("Provide a game title or Steam AppID to search SteamGridDB.".to_string());
    }

    let matched_game = if let Ok(appid) = trimmed.parse::<u64>() {
        let game_url = format!("https://www.steamgriddb.com/api/v2/games/steam/{}", appid);
        let resp = sgdb_get_json(&game_url, &api_key).await?;
        let data = resp.get("data").cloned().unwrap_or(Value::Null);
        let sgdb_id = data.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        if sgdb_id == 0 {
            return Ok(SteamGridDbSearchResult {
                has_api_key: true,
                message: Some(format!("No SteamGridDB match found for Steam AppID {}.", appid)),
                ..Default::default()
            });
        }
        SteamGridDbGameInfo {
            id: sgdb_id,
            name: data
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(&trimmed)
                .to_string(),
            verified: data
                .get("verified")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        }
    } else {
        let search_url = format!(
            "https://www.steamgriddb.com/api/v2/search/autocomplete/{}",
            urlencoding::encode(&trimmed)
        );
        let resp = sgdb_get_json(&search_url, &api_key).await?;
        let Some(first) = resp
            .get("data")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
        else {
            return Ok(SteamGridDbSearchResult {
                has_api_key: true,
                message: Some(format!("No SteamGridDB match found for '{}'.", trimmed)),
                ..Default::default()
            });
        };
        SteamGridDbGameInfo {
            id: first.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
            name: first
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(&trimmed)
                .to_string(),
            verified: first
                .get("verified")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        }
    };

    let grids_url = format!(
        "https://www.steamgriddb.com/api/v2/grids/game/{}?dimensions=600x900&types=static,animated",
        matched_game.id
    );
    let heroes_url = format!(
        "https://www.steamgriddb.com/api/v2/heroes/game/{}?types=static",
        matched_game.id
    );
    let icons_url = format!(
        "https://www.steamgriddb.com/api/v2/icons/game/{}?types=static",
        matched_game.id
    );

    let (grids_res, heroes_res, icons_res) = tokio::join!(
        sgdb_get_json(&grids_url, &api_key),
        sgdb_get_json(&heroes_url, &api_key),
        sgdb_get_json(&icons_url, &api_key),
    );

    let grids = grids_res
        .as_ref()
        .map(|v| parse_sgdb_assets(v, 18))
        .unwrap_or_default();
    let heroes = heroes_res
        .as_ref()
        .map(|v| parse_sgdb_assets(v, 10))
        .unwrap_or_default();
    let icons = icons_res
        .as_ref()
        .map(|v| parse_sgdb_assets(v, 12))
        .unwrap_or_default();

    crate::trim_process_working_set();
    Ok(SteamGridDbSearchResult {
        has_api_key: true,
        matched_game: Some(matched_game),
        grids,
        heroes,
        icons,
        message: None,
    })
}

async fn download_binary_asset(url: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    let parsed = crate::validate_external_http_url(url)?;
    let client = duke_core::net_doh::build_doh_client_with_timeouts(
        parsed.as_str(),
        Duration::from_secs(10),
        Duration::from_secs(20),
        true,
    )
    .await;

    let resp = client
        .get(parsed.as_str())
        .send()
        .await
        .map_err(|e| format!("Failed downloading artwork asset: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!(
            "Artwork asset URL returned HTTP {}",
            resp.status().as_u16()
        ));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Failed reading artwork bytes: {}", e))?;
    if bytes.len() > max_bytes {
        return Err(format!(
            "Artwork file exceeds maximum allowed size ({} bytes).",
            max_bytes
        ));
    }
    Ok(bytes.to_vec())
}

#[tauri::command]
pub(crate) async fn apply_custom_artwork(
    app: AppHandle,
    game_id: String,
    cover_url: Option<String>,
    hero_url: Option<String>,
    icon_url: Option<String>,
) -> Result<crate::GameEntry, String> {
    let clean_cover = cover_url.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let clean_hero = hero_url.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let clean_icon = icon_url.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

    let mut downloaded_cover_uri: Option<String> = None;
    if let Some(ref c_url) = clean_cover {
        if c_url.starts_with("data:image/") {
            downloaded_cover_uri = Some(c_url.clone());
        } else {
            let _ = crate::validate_external_http_url(c_url)?;
            if let Some(data_uri) = crate::fetch_verified_image_data_uri(c_url).await {
                downloaded_cover_uri = Some(data_uri);
            } else {
                downloaded_cover_uri = Some(c_url.clone());
            }
        }
    }

    if let Some(ref h_url) = clean_hero {
        let _ = crate::validate_external_http_url(h_url)?;
    }

    let mut saved_icon_path: Option<String> = None;
    if let Some(ref i_url) = clean_icon {
        let raw_bytes = download_binary_asset(i_url, 2 * 1024 * 1024).await?;
        let ico_bytes = if raw_bytes.len() >= 4 && raw_bytes[0..4] == [0, 0, 1, 0] {
            raw_bytes
        } else if let Some(wrapped) = wrap_png_bytes_in_ico(&raw_bytes) {
            wrapped
        } else {
            return Err("Selected icon asset must be a valid .ico or .png file.".to_string());
        };

        let covers_dir = crate::get_covers_dir_standalone()?;
        let safe_id: String = game_id
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_')
            .take(80)
            .collect();
        let ico_file = covers_dir.join(format!("{}.ico", if safe_id.is_empty() { "custom_game" } else { &safe_id }));
        crate::atomic_write_bytes(&ico_file, &ico_bytes)?;
        saved_icon_path = Some(ico_file.to_string_lossy().to_string());
    }

    let updated_entry = tokio::task::spawn_blocking(move || -> Result<crate::GameEntry, String> {
        let mut library = crate::load_library_standalone()?;
        let Some(entry) = library
            .iter_mut()
            .find(|g| g.id == game_id || g.exe_path.eq_ignore_ascii_case(&game_id))
        else {
            return Err(format!("Game '{}' not found in library.", game_id));
        };

        if let Some(cov) = downloaded_cover_uri {
            entry.cover_url = cov;
        }
        if let Some(hero) = clean_hero {
            entry.hero_url = Some(hero);
        }
        if let Some(icon_p) = saved_icon_path {
            entry.icon_path = Some(icon_p);
        }

        let cloned = entry.clone();
        crate::save_library_standalone(&library)?;
        Ok(cloned)
    })
    .await
    .map_err(|e| format!("Failed saving custom artwork: {}", e))??;

    if let Ok(full_lib) = tokio::task::spawn_blocking(crate::load_library_standalone)
        .await
        .unwrap_or_else(|_| Ok(Vec::new()))
    {
        let _ = app.emit("library-updated", &full_lib);
    }

    crate::trim_process_working_set();
    Ok(updated_entry)
}

fn resolve_desktop_dir() -> Result<PathBuf, String> {
    let profile = std::env::var("USERPROFILE").unwrap_or_default();
    if let Some(shell_desktop) = duke_core::win32_ffi::reg_read_string(
        false,
        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\User Shell Folders",
        "Desktop",
    ) {
        let expanded = if !profile.is_empty() {
            shell_desktop.replace("%USERPROFILE%", &profile)
        } else {
            shell_desktop
        };
        let p = PathBuf::from(expanded);
        if p.exists() {
            return Ok(p);
        }
    }
    if !profile.is_empty() {
        let fallback = PathBuf::from(profile).join("Desktop");
        if fallback.exists() {
            return Ok(fallback);
        }
    }
    Err("Could not locate Windows Desktop directory.".to_string())
}

fn escape_ps_single_quoted(raw: &str) -> String {
    raw.replace('\'', "''")
}

#[tauri::command]
pub(crate) async fn create_desktop_shortcut_with_icon(
    game_id: String,
) -> Result<String, String> {
    let entry = tokio::task::spawn_blocking(move || -> Result<crate::GameEntry, String> {
        let library = crate::load_library_standalone()?;
        library
            .into_iter()
            .find(|g| g.id == game_id || g.exe_path.eq_ignore_ascii_case(&game_id))
            .ok_or_else(|| format!("Game '{}' not found in library.", game_id))
    })
    .await
    .map_err(|e| format!("Failed loading library: {}", e))??;

    let exe_path = Path::new(&entry.exe_path);
    if !exe_path.exists() {
        return Err(format!(
            "Game executable does not exist at '{}'.",
            entry.exe_path
        ));
    }

    let desktop_dir = resolve_desktop_dir()?;
    let safe_name: String = entry
        .title
        .chars()
        .filter(|&c| !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect::<String>()
        .trim()
        .to_string();
    let lnk_filename = format!(
        "{}.lnk",
        if safe_name.is_empty() { "DukeGame" } else { &safe_name }
    );
    let lnk_path = desktop_dir.join(lnk_filename);
    let working_dir = exe_path
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| entry.install_dir.clone());

    let icon_location = entry
        .icon_path
        .as_deref()
        .filter(|p| Path::new(p).exists())
        .unwrap_or(&entry.exe_path)
        .to_string();
    let args = entry.launch_args.clone().unwrap_or_default();

    let script = format!(
        "$ws = New-Object -ComObject WScript.Shell; \
         $s = $ws.CreateShortcut('{}'); \
         $s.TargetPath = '{}'; \
         $s.WorkingDirectory = '{}'; \
         $s.Arguments = '{}'; \
         $s.IconLocation = '{}'; \
         $s.Save();",
        escape_ps_single_quoted(&lnk_path.to_string_lossy()),
        escape_ps_single_quoted(&entry.exe_path),
        escape_ps_single_quoted(&working_dir),
        escape_ps_single_quoted(&args),
        escape_ps_single_quoted(&icon_location),
    );

    let out = tokio::task::spawn_blocking(move || {
        Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                &script,
            ])
            .creation_flags(crate::CREATE_NO_WINDOW)
            .output()
    })
    .await
    .map_err(|e| format!("Shortcut creator task failed: {}", e))?
    .map_err(|e| format!("Failed launching PowerShell shortcut helper: {}", e))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("Failed creating desktop shortcut: {}", stderr.trim()));
    }

    Ok(lnk_path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_png_signature_inside_valid_ico_header() {
        let fake_png = b"\x89PNG\r\n\x1a\n0123456789ABCDEF0123456789";
        let wrapped = wrap_png_bytes_in_ico(fake_png).unwrap();
        assert_eq!(&wrapped[0..6], &[0, 0, 1, 0, 1, 0]);
        assert_eq!(wrapped.len(), 22 + fake_png.len());
        assert_eq!(&wrapped[22..], fake_png);
        assert!(wrap_png_bytes_in_ico(b"NOT_A_PNG").is_none());
    }
}
