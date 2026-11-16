use reqwest::header::{ACCEPT, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tauri::AppHandle;

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct WebCoverCandidate {
    pub url: String,
    pub thumb: String,
    pub source: String,
    pub label: String,
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

    duke_core::win32_ffi::trim_current_working_set();
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

    let mut resp = client
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
    if let Some(cl) = resp.content_length() {
        if cl > max_bytes as u64 {
            return Err(format!(
                "Artwork file exceeds maximum allowed size ({} bytes).",
                max_bytes
            ));
        }
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("Failed reading artwork bytes: {}", e))?
    {
        if out.len().saturating_add(chunk.len()) > max_bytes {
            return Err(format!(
                "Artwork file exceeds maximum allowed size ({} bytes).",
                max_bytes
            ));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
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
    let cover_updated = clean_cover.is_some();
    let clean_hero = hero_url.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let clean_icon = icon_url.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

    let covers_dir = crate::get_covers_dir_standalone()?;
    let sidebar_icons_dir = crate::sidebar_icons::get_sidebar_icons_dir_standalone()?;
    let safe_id: String = crate::sidebar_icons::safe_sidebar_icon_stem(&game_id);
    let safe_id = if safe_id.is_empty() {
        "custom_game".to_string()
    } else {
        safe_id
    };

    let mut resolved_cover_path: Option<String> = None;
    if let Some(ref c_url) = clean_cover {
        let mut raw_cover_bytes: Option<(Vec<u8>, &'static str)> = None;
        if let Some(b64_part) = c_url.strip_prefix("data:image/") {
            if let Some((_, payload)) = b64_part.split_once(";base64,") {
                use base64::Engine;
                if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(payload.trim()) {
                    if let Some((ext, _)) = verify_image_bytes(&decoded) {
                        raw_cover_bytes = Some((decoded, ext));
                    }
                }
            }
        } else if c_url.starts_with("https://") || c_url.starts_with("http://") {
            let _ = crate::validate_external_http_url(c_url)?;
            raw_cover_bytes = download_verified_image_doh(c_url, None, MIN_VALID_COVER_BYTES).await;
        } else {
            let local_path = Path::new(c_url);
            if local_path.is_file() {
                if let Ok(bytes) = std::fs::read(local_path) {
                    if let Some((ext, _)) = verify_image_bytes(&bytes) {
                        raw_cover_bytes = Some((bytes, ext));
                    } else {
                        return Err("Selected local cover file is not a valid PNG, JPEG, or WebP image.".to_string());
                    }
                }
            }
        }

        if let Some((bytes, ext)) = raw_cover_bytes {
            clear_cached_cover_for_id(&covers_dir, &safe_id);
            let dest_path = covers_dir.join(format!("{}.{}", safe_id, ext));
            crate::atomic_write_bytes(&dest_path, &bytes)?;
            let _ = ensure_sidebar_icon_for_cover_bytes(&sidebar_icons_dir, &safe_id, &bytes, true);
            resolved_cover_path = Some(dest_path.to_string_lossy().to_string());
        } else if c_url.starts_with("https://") || c_url.starts_with("http://") {
            resolved_cover_path = Some(c_url.clone());
        } else {
            return Err("Failed verifying selected cover image.".to_string());
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

        let ico_file = covers_dir.join(format!("{}.ico", safe_id));
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

        if let Some(cov) = resolved_cover_path {
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

    crate::sidebar_icons::invalidate_sidebar_icon(&app, &updated_entry.id);

    if let Ok(full_lib) = tokio::task::spawn_blocking(crate::load_library_standalone)
        .await
        .unwrap_or_else(|_| Ok(Vec::new()))
    {
        crate::emit_if_alive(&app, "library-updated", &full_lib);
    }

    if cover_updated {
        crate::emit_if_alive(
            &app,
            "cover-updated",
            serde_json::json!({
                "gameId": &updated_entry.id,
                "coverUrl": &updated_entry.cover_url,
            }),
        );
    }

    duke_core::win32_ffi::trim_current_working_set();
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

const COVER_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const COVER_READ_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_COVER_DOWNLOAD_BYTES: usize = 8 * 1024 * 1024;
const MIN_VALID_COVER_BYTES: usize = 512;
const MIN_WEB_FALLBACK_IMAGE_BYTES: usize = 2048;

static COVER_HTTP_SEMAPHORE: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
    std::sync::OnceLock::new();

pub(crate) fn get_cover_http_semaphore() -> &'static std::sync::Arc<tokio::sync::Semaphore> {
    COVER_HTTP_SEMAPHORE.get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(4)))
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CoverResolveRequest {
    pub cache_id: String,
    pub title: String,
    pub exe_path: Option<String>,
    pub platform_id: Option<u16>,
    pub platform_hint: Option<String>,
    pub steam_app_id: Option<u32>,
    pub existing_cover_url: Option<String>,
    pub steamgriddb_api_key: String,
    pub sidebar_icon_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct CoverResolveOutcome {
    pub cover_path: PathBuf,
    pub sidebar_icon_path: Option<PathBuf>,
    #[allow(dead_code)]
    pub source_tier: &'static str,
}

/// Validates raw image bytes via `image::guess_format`, accepting JPEG, PNG, and WebP.
pub(crate) fn verify_image_bytes(bytes: &[u8]) -> Option<(&'static str, image::ImageFormat)> {
    if bytes.len() < MIN_VALID_COVER_BYTES || bytes.len() > MAX_COVER_DOWNLOAD_BYTES {
        return None;
    }
    let fmt = image::guess_format(bytes).ok()?;
    let ext = match fmt {
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::Png => "png",
        image::ImageFormat::WebP => "webp",
        _ => return None,
    };
    Some((ext, fmt))
}

/// Validates web fallback cover art dimensions (`>= 200x200`) and box-art aspect ratio (`0.55..=1.15`).
pub(crate) fn verify_web_cover_dimensions(bytes: &[u8]) -> Option<(&'static str, u32, u32)> {
    let (ext, _) = verify_image_bytes(bytes)?;
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let (w, h) = reader.into_dimensions().ok()?;
    if w < 200 || h < 200 {
        return None;
    }
    let ratio = w as f32 / h as f32;
    if !(0.55..=1.15).contains(&ratio) {
        return None;
    }
    Some((ext, w, h))
}

/// Strips region/revision tags like `(USA)`, `(Europe)`, `(Japan)`, `(Rev 1)`, `[!]` from a title.
pub(crate) fn strip_region_revision_tags(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut paren_depth = 0u32;
    let mut bracket_depth = 0u32;
    for ch in raw.chars() {
        match ch {
            '(' => paren_depth = paren_depth.saturating_add(1),
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '[' => bracket_depth = bracket_depth.saturating_add(1),
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            _ if paren_depth == 0 && bracket_depth == 0 => out.push(ch),
            _ => {}
        }
    }
    let collapsed: String = out
        .replace(['™', '®'], "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let trimmed = collapsed.trim().trim_matches(|c| c == '-' || c == '_').trim();
    if trimmed.is_empty() {
        crate::retro_catalog::normalize_retro_base_title(raw)
    } else {
        trimmed.to_string()
    }
}

fn normalize_cover_title_tokens(raw: &str) -> String {
    strip_region_revision_tags(raw)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_version_token(lower: &str) -> bool {
    let trimmed = lower.trim_start_matches('v');
    let has_digit = trimmed.chars().any(|c| c.is_ascii_digit());
    let has_dot = trimmed.contains('.');
    let valid_chars = trimmed
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == '_');
    (lower.starts_with('v') && has_digit && trimmed.chars().next().is_some_and(|c| c.is_ascii_digit()))
        || (has_digit && has_dot && valid_chars)
}

fn is_scene_or_release_tag(lower: &str) -> bool {
    if matches!(
        lower,
        "steam"
            | "steamrip"
            | "steamunlocked"
            | "fitgirl"
            | "dodi"
            | "elamigos"
            | "codex"
            | "rune"
            | "tenoke"
            | "skidrow"
            | "plaza"
            | "cpy"
            | "empress"
            | "goldberg"
            | "gog"
            | "repack"
            | "portable"
            | "p2p"
            | "cracked"
            | "preinstalled"
            | "x64"
            | "x86"
            | "win64"
            | "win32"
            | "goty"
            | "usa"
            | "europe"
            | "japan"
            | "world"
    ) {
        return true;
    }
    if let Some(rest) = lower.strip_prefix("multi") {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    false
}

/// Cleans game titles for cover queries: strips scene/release tags, version numbers (`v1.2.3`),
/// `(USA)` / `(Europe)` / `(Disc 1)` / `[!]`, `GOTY Edition`, hyphens/underscores, and trailing ellipsis.
pub(crate) fn clean_cover_query(raw: &str) -> String {
    let stripped = strip_region_revision_tags(raw);
    let mut normalized = stripped
        .trim_end_matches("...")
        .trim_end_matches('\u{2026}')
        .replace(['_', '-'], " ");

    // Replace separator dots in dotted filenames while keeping version strings intact until token filtering.
    if !normalized.contains(' ') && normalized.matches('.').count() >= 2 {
        normalized = normalized.replace('.', " ");
    }

    let raw_tokens: Vec<&str> = normalized.split_whitespace().collect();
    let mut kept: Vec<String> = Vec::with_capacity(raw_tokens.len());
    let mut i = 0usize;
    while i < raw_tokens.len() {
        let tok = raw_tokens[i].trim_matches(|c: char| matches!(c, ',' | ';' | '|'));
        let lower = tok.to_ascii_lowercase();
        let lower_alpha = lower.trim_end_matches("...");

        // Multi-word edition suffixes ("Game of the Year Edition", "GOTY Edition", etc.)
        if i + 4 < raw_tokens.len()
            && lower == "game"
            && raw_tokens[i + 1].eq_ignore_ascii_case("of")
            && raw_tokens[i + 2].eq_ignore_ascii_case("the")
            && raw_tokens[i + 3].eq_ignore_ascii_case("year")
            && raw_tokens[i + 4].eq_ignore_ascii_case("edition")
        {
            i += 5;
            continue;
        }
        if i + 3 < raw_tokens.len()
            && lower == "game"
            && raw_tokens[i + 1].eq_ignore_ascii_case("of")
            && raw_tokens[i + 2].eq_ignore_ascii_case("the")
            && raw_tokens[i + 3].eq_ignore_ascii_case("year")
        {
            i += 4;
            continue;
        }
        if i + 1 < raw_tokens.len()
            && raw_tokens[i + 1].eq_ignore_ascii_case("edition")
            && matches!(
                lower_alpha,
                "goty" | "definitive" | "deluxe" | "complete" | "ultimate" | "enhanced" | "gold" | "special" | "collector's" | "collectors"
            )
        {
            i += 2;
            continue;
        }
        if matches!(lower.as_str(), "build" | "ver" | "rev" | "disc")
            && raw_tokens.get(i + 1).is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()))
        {
            i += 2;
            continue;
        }

        if tok.is_empty()
            || is_version_token(lower_alpha)
            || is_scene_or_release_tag(lower_alpha)
            || (lower_alpha == "edition" && !kept.is_empty())
        {
            // Once trailing release tags begin (e.g. "-Steam..." or "v1.2.3 REPACK"), keep cleaning safely.
            i += 1;
            continue;
        }

        kept.push(tok.to_string());
        i += 1;
    }

    let joined = kept.join(" ").trim().to_string();
    if joined.is_empty() {
        stripped
    } else {
        joined
    }
}

/// Computes standard Jaro-Winkler similarity in `0.0..=1.0` after normalizing query tokens.
pub(crate) fn jaro_winkler_similarity(a: &str, b: &str) -> f64 {
    let s1_str = normalize_cover_title_tokens(a);
    let s2_str = normalize_cover_title_tokens(b);
    if s1_str.is_empty() || s2_str.is_empty() {
        return 0.0;
    }
    if s1_str == s2_str {
        return 1.0;
    }
    let s1: Vec<char> = s1_str.chars().collect();
    let s2: Vec<char> = s2_str.chars().collect();
    let len1 = s1.len();
    let len2 = s2.len();
    let match_distance = len1.max(len2) / 2;
    let match_distance = match_distance.saturating_sub(1);

    let mut s1_matches = vec![false; len1];
    let mut s2_matches = vec![false; len2];
    let mut matches = 0usize;

    for i in 0..len1 {
        let start = i.saturating_sub(match_distance);
        let end = (i + match_distance + 1).min(len2);
        for j in start..end {
            if s2_matches[j] || s1[i] != s2[j] {
                continue;
            }
            s1_matches[i] = true;
            s2_matches[j] = true;
            matches += 1;
            break;
        }
    }
    if matches == 0 {
        return 0.0;
    }

    let mut transpositions = 0usize;
    let mut k = 0usize;
    for i in 0..len1 {
        if !s1_matches[i] {
            continue;
        }
        while k < len2 && !s2_matches[k] {
            k += 1;
        }
        if k < len2 && s1[i] != s2[k] {
            transpositions += 1;
        }
        k += 1;
    }

    let m = matches as f64;
    let jaro = (m / len1 as f64 + m / len2 as f64 + (m - (transpositions as f64 / 2.0)) / m) / 3.0;
    if jaro < 0.7 {
        return jaro;
    }
    let prefix = s1
        .iter()
        .zip(s2.iter())
        .take(4)
        .take_while(|(c1, c2)| c1 == c2)
        .count();
    jaro + (prefix as f64 * 0.1 * (1.0 - jaro))
}

pub(crate) fn titles_match_for_cover(query_title: &str, candidate_title: &str) -> bool {
    let q_clean = clean_cover_query(query_title);
    let c_clean = clean_cover_query(candidate_title);
    let q = normalize_cover_title_tokens(&q_clean);
    let c = normalize_cover_title_tokens(&c_clean);
    if q.is_empty() || c.is_empty() {
        return false;
    }
    if q == c {
        return true;
    }
    if jaro_winkler_similarity(&q, &c) >= 0.78 {
        return true;
    }
    let q_words: Vec<&str> = q.split_whitespace().collect();
    let c_words: Vec<&str> = c.split_whitespace().collect();
    if q_words.len() >= 2 && c_words.len() >= 2 {
        let matched = q_words
            .iter()
            .filter(|w| c_words.contains(w))
            .count();
        let min_len = q_words.len().min(c_words.len());
        let max_len = q_words.len().max(c_words.len());
        return matched == min_len && matched * 2 >= max_len;
    }
    false
}

pub(crate) fn build_steam_cdn_urls(app_id: u32) -> [String; 4] {
    [
        format!(
            "https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/{}/library_600x900_2x.jpg",
            app_id
        ),
        format!(
            "https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/{}/library_600x900.jpg",
            app_id
        ),
        format!(
            "https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/{}/header.jpg",
            app_id
        ),
        format!(
            "https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/{}/capsule_616x353.jpg",
            app_id
        ),
    ]
}

fn find_cached_cover_on_disk(covers_dir: &Path, keys: &[String]) -> Option<PathBuf> {
    const EXTS: &[&str] = &["jpg", "jpeg", "png", "webp"];
    for key in keys {
        let stem = crate::sidebar_icons::safe_sidebar_icon_stem(key);
        if stem.is_empty() {
            continue;
        }
        for ext in EXTS {
            let candidate = covers_dir.join(format!("{}.{}", stem, ext));
            let Ok(meta) = std::fs::metadata(&candidate) else {
                continue;
            };
            if !meta.is_file()
                || meta.len() < MIN_VALID_COVER_BYTES as u64
                || meta.len() > MAX_COVER_DOWNLOAD_BYTES as u64
            {
                continue;
            }
            if let Ok(bytes) = std::fs::read(&candidate) {
                if verify_image_bytes(&bytes).is_some() {
                    return Some(candidate);
                }
                let _ = std::fs::remove_file(&candidate);
            }
        }
    }
    None
}

pub(crate) fn clear_cached_cover_for_id(covers_dir: &Path, cache_id: &str) {
    let stem = crate::sidebar_icons::safe_sidebar_icon_stem(cache_id);
    if stem.is_empty() {
        return;
    }
    for ext in ["jpg", "jpeg", "png", "webp", "cache"] {
        let candidate = covers_dir.join(format!("{}.{}", stem, ext));
        if candidate.is_file() {
            let _ = std::fs::remove_file(&candidate);
        }
    }
}

fn ensure_sidebar_icon_for_cover_bytes(
    sidebar_icons_dir: &Path,
    icon_id: &str,
    cover_bytes: &[u8],
    force_refresh: bool,
) -> Option<PathBuf> {
    let dest = crate::sidebar_icons::cached_sidebar_icon_path(sidebar_icons_dir, icon_id);
    if !force_refresh {
        if let Ok(meta) = std::fs::metadata(&dest) {
            if meta.is_file() && meta.len() > 8 {
                return Some(dest);
            }
        }
    }
    let (rgba, w, h) = crate::sidebar_icons::decode_image_bytes_to_rgba(cover_bytes)?;
    let (sq, side) = crate::sidebar_icons::center_crop_square_rgba(&rgba, w, h)?;
    let resized = crate::sidebar_icons::lanczos3_resize_rgba(
        &sq,
        side,
        side,
        crate::sidebar_icons::SIDEBAR_ICON_SIZE,
        crate::sidebar_icons::SIDEBAR_ICON_SIZE,
    )?;
    let png_bytes = crate::sidebar_icons::encode_rgba_png(
        &resized,
        crate::sidebar_icons::SIDEBAR_ICON_SIZE,
        crate::sidebar_icons::SIDEBAR_ICON_SIZE,
    )
    .ok()?;
    crate::sidebar_icons::atomic_write_png_bytes(&dest, &png_bytes).ok()?;
    Some(dest)
}

async fn download_verified_image_doh(
    url: &str,
    referer: Option<&str>,
    min_bytes: usize,
) -> Option<(Vec<u8>, &'static str)> {
    let trimmed = url.trim();
    if !trimmed.starts_with("https://") && !trimmed.starts_with("http://") {
        return None;
    }
    let _permit = get_cover_http_semaphore().clone().acquire_owned().await.ok()?;
    let client = duke_core::net_doh::build_doh_client_with_timeouts(
        trimmed,
        COVER_CONNECT_TIMEOUT,
        COVER_READ_TIMEOUT,
        true,
    )
    .await;

    let mut req = client.get(trimmed);
    if let Some(rf) = referer {
        req = req.header(reqwest::header::REFERER, rf);
    }
    let mut resp = req.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    if let Some(cl) = resp.content_length() {
        if (cl as usize) > MAX_COVER_DOWNLOAD_BYTES || (cl as usize) < min_bytes {
            return None;
        }
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await.ok()? {
        if out.len().saturating_add(chunk.len()) > MAX_COVER_DOWNLOAD_BYTES {
            return None;
        }
        out.extend_from_slice(&chunk);
    }
    if out.len() < min_bytes {
        return None;
    }
    let (ext, _) = verify_image_bytes(&out)?;
    Some((out, ext))
}

async fn search_steam_app_ids_doh(clean_title: &str) -> Vec<u32> {
    if clean_title.trim().chars().count() < 2 {
        return Vec::new();
    }
    let encoded = urlencoding::encode(clean_title.trim());
    let url = format!(
        "https://store.steampowered.com/api/storesearch/?term={}&cc=us&l=en",
        encoded
    );
    let Ok(_permit) = get_cover_http_semaphore().clone().acquire_owned().await else {
        return Vec::new();
    };
    let client = duke_core::net_doh::build_doh_client_with_timeouts(
        &url,
        COVER_CONNECT_TIMEOUT,
        COVER_READ_TIMEOUT,
        true,
    )
    .await;
    let Ok(resp) = client.get(&url).send().await else {
        return Vec::new();
    };
    if !resp.status().is_success() {
        return Vec::new();
    }
    let Ok(json) = resp.json::<Value>().await else {
        return Vec::new();
    };
    let Some(items) = json.get("items").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut scored: Vec<(f64, u32)> = Vec::new();
    let q_clean = clean_cover_query(clean_title);
    for item in items.iter().take(10) {
        let id = item.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if id > 0 {
            let sim = jaro_winkler_similarity(&q_clean, name);
            if sim >= 0.78 || titles_match_for_cover(&q_clean, name) {
                scored.push((sim, id));
            }
        }
    }
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.into_iter().map(|(_, id)| id).collect()
}

fn resolve_target_platform_spec(
    platform_id: Option<u16>,
    platform_hint: Option<&str>,
) -> Option<&'static crate::platforms::PlatformSpec> {
    if let Some(pid) = platform_id {
        if pid != crate::platforms::PLATFORM_PC {
            return Some(crate::platforms::platform_by_id(pid));
        }
    }
    if let Some(hint) = platform_hint {
        let lower = hint.trim().to_ascii_lowercase();
        if let Some(spec) = crate::platforms::platform_from_slug_or_name(&lower) {
            if spec.id != crate::platforms::PLATFORM_PC {
                return Some(spec);
            }
        }
        for spec in crate::platforms::PLATFORMS {
            if spec.id != crate::platforms::PLATFORM_PC
                && (lower.contains(spec.slug) || lower.contains(&spec.short_name.to_ascii_lowercase()))
            {
                return Some(spec);
            }
        }
    }
    None
}

/// Builds candidate Libretro Thumbnails URLs in strict order (`Named_Boxarts` -> `Named_Snaps` -> `Named_Titles`)
/// with stripped region/revision tags and sanitized punctuation (`&`, `:`, `/` -> `_`).
pub(crate) fn build_libretro_fuzzy_candidate_urls(
    spec: &'static crate::platforms::PlatformSpec,
    raw_title: &str,
) -> Vec<String> {
    let mut systems: Vec<&str> = Vec::new();
    if let Some(sys) = spec.thumb_system {
        systems.push(sys);
    }
    for &alt in spec.alt_thumb_systems {
        if !systems.contains(&alt) {
            systems.push(alt);
        }
    }
    if systems.is_empty() {
        return Vec::new();
    }

    let base_clean = clean_cover_query(raw_title);
    let mut title_variants: Vec<String> = Vec::new();
    let raw_trimmed = strip_region_revision_tags(raw_title);
    if !raw_trimmed.is_empty() && raw_trimmed != base_clean {
        title_variants.push(crate::retro_catalog::sanitize_libretro_thumb_name(&raw_trimmed));
    }
    if !base_clean.is_empty() {
        for suffix in [" (USA)", " (World)", " (Europe)", " (Japan)", ""] {
            let candidate = crate::retro_catalog::sanitize_libretro_thumb_name(&format!(
                "{}{}",
                base_clean, suffix
            ));
            if !title_variants.contains(&candidate) {
                title_variants.push(candidate);
            }
        }
        if base_clean.contains(':') || base_clean.contains('&') {
            let hyphen_variant = crate::retro_catalog::sanitize_libretro_thumb_name(&format!(
                "{} (USA)",
                base_clean.replace(" - ", "_ ")
            ));
            if !title_variants.contains(&hyphen_variant) {
                title_variants.push(hyphen_variant);
            }
        }
    }

    let mut urls = Vec::new();
    for folder in ["Named_Boxarts", "Named_Snaps", "Named_Titles"] {
        for sys in &systems {
            let sys_enc = urlencoding::encode(sys);
            for variant in &title_variants {
                let png_filename = format!("{}.png", variant);
                let file_enc = urlencoding::encode(&png_filename);
                let url = format!(
                    "https://thumbnails.libretro.com/{}/{}/{}",
                    sys_enc, folder, file_enc
                );
                if !urls.contains(&url) {
                    urls.push(url);
                }
            }
        }
    }
    urls
}

/// Extracts candidate HTTPS `.jpg`/`.jpeg`/`.png`/`.webp` URLs from DuckDuckGo HTML/i.js JSON and Bing image HTML.
pub(crate) fn extract_ddg_image_urls_from_html(html: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut push_if_valid = |candidate_raw: &str| {
        let mut clean = candidate_raw
            .trim()
            .trim_matches(|c| matches!(c, '"' | '\'' | '<' | '>' | ',' | ')' | ']'))
            .replace("&amp;", "&")
            .replace("&quot;", "")
            .replace("\\/", "/");
        if clean.starts_with("//") {
            clean = format!("https:{}", clean);
        }
        if !clean.starts_with("https://") {
            return;
        }
        let lower = clean.to_ascii_lowercase();
        let authority = lower
            .strip_prefix("https://")
            .unwrap_or("")
            .split(['/', '?', '#'])
            .next()
            .unwrap_or("");
        let host = authority.split(':').next().unwrap_or("");
        let is_ddg_host = host == "duckduckgo.com" || host.ends_with(".duckduckgo.com");
        if is_ddg_host && !lower.contains("iu/?u=https") {
            return;
        }
        if (host.ends_with("bing.net") || host.ends_with("bing.com") || host.ends_with("gstatic.com"))
            && !lower.contains("murl")
            && !lower.contains(".jpg")
            && !lower.contains(".png")
            && !lower.contains(".webp")
        {
            return;
        }
        if lower.contains("favicon")
            || lower.contains(".ico")
            || lower.contains(".svg")
            || lower.contains("doubleclick")
            || lower.contains("bing.com/th?")
        {
            return;
        }
        let path_part = lower.split('?').next().unwrap_or(&lower);
        let is_img_ext = path_part.ends_with(".jpg")
            || path_part.ends_with(".jpeg")
            || path_part.ends_with(".png")
            || path_part.ends_with(".webp")
            || lower.contains("iu/?u=https");
        if is_img_ext && !results.contains(&clean) {
            results.push(clean);
        }
    };

    // 1. DuckDuckGo `uddg=` redirect params
    let mut pos = 0usize;
    while let Some(rel) = html.get(pos..).and_then(|s| s.find("uddg=")) {
        let start = pos + rel + 5;
        let Some(rest) = html.get(start..) else { break; };
        let end = rest
            .find(['&', '"', '\'', '<', '>', ' '])
            .map(|i| start + i)
            .unwrap_or(html.len());
        if let Some(slice) = html.get(start..end) {
            if let Ok(decoded) = urlencoding::decode(slice) {
                push_if_valid(&decoded);
            }
        }
        pos = end.saturating_add(1).max(start + 1).min(html.len());
    }

    // 2. Bing `"murl":"https://..."` / `&quot;murl&quot;:&quot;https://...&quot;` attributes
    for marker in ["\"murl\":\"", "&quot;murl&quot;:&quot;"] {
        let mut mpos = 0usize;
        while let Some(rel) = html.get(mpos..).and_then(|s| s.find(marker)) {
            let start = mpos + rel + marker.len();
            let Some(rest) = html.get(start..) else { break; };
            let end = rest
                .find('"')
                .or_else(|| rest.find("&quot;"))
                .map(|i| start + i)
                .unwrap_or(html.len());
            if let Some(slice) = html.get(start..end) {
                push_if_valid(slice);
            }
            mpos = end.saturating_add(1).max(start + 1).min(html.len());
        }
    }

    // 3. Direct image URLs inside HTML or DuckDuckGo `i.js` JSON (`"image":"https:\/\/..."`)
    let unescaped = html.replace("\\/", "/");
    let mut spos = 0usize;
    while let Some(rel) = unescaped.get(spos..).and_then(|s| s.find("https://")) {
        let start = spos + rel;
        let Some(rest) = unescaped.get(start..) else { break; };
        let end = rest
            .find(['"', '\'', '<', '>', ' ', '\n', '\r', '\\'])
            .map(|i| start + i)
            .unwrap_or(unescaped.len());
        if let Some(slice) = unescaped.get(start..end) {
            push_if_valid(slice);
        }
        spos = end.saturating_add(1).max(start + 1).min(unescaped.len());
    }
    results
}

fn extract_ddg_vqd_token(html: &str) -> Option<String> {
    for pat in ["vqd=\"", "vqd='", "vqd="] {
        if let Some(idx) = html.find(pat) {
            let rest = &html[idx + pat.len()..];
            let tok: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if tok.len() >= 6 {
                return Some(tok);
            }
        }
    }
    None
}

async fn doh_fetch_text_body(url: &str, referer: Option<&str>) -> Option<String> {
    let _permit = get_cover_http_semaphore().clone().acquire_owned().await.ok()?;
    let client = duke_core::net_doh::build_doh_client_with_timeouts(
        url,
        COVER_CONNECT_TIMEOUT,
        COVER_READ_TIMEOUT,
        true,
    )
    .await;
    let mut req = client.get(url);
    if let Some(rf) = referer {
        req = req.header(reqwest::header::REFERER, rf);
    }
    let resp = req.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.text().await.ok()
}

fn push_unique_url(out: &mut Vec<String>, u: String) {
    if !out.contains(&u) {
        out.push(u);
    }
}

async fn collect_doh_web_image_urls(clean_title: &str, platform_hint: Option<&str>) -> Vec<String> {
    let clean = clean_cover_query(clean_title);
    if clean.trim().chars().count() < 2 {
        return Vec::new();
    }
    let platform_kw = platform_hint
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("pc"))
        .unwrap_or("");
    let query = if platform_kw.is_empty() {
        format!("\"{}\" game cover box art", clean.trim())
    } else {
        format!("\"{}\" {} game cover box art", clean.trim(), platform_kw)
    };
    let encoded_q = urlencoding::encode(&query);

    let mut candidate_urls: Vec<String> = Vec::new();

    // Endpoint A: DuckDuckGo Images `i.js` JSON via DoH
    let ddg_boot_url = format!(
        "https://duckduckgo.com/?q={}&iax=images&ia=images",
        encoded_q
    );
    if let Some(boot_html) = doh_fetch_text_body(&ddg_boot_url, Some("https://duckduckgo.com/")).await {
        if let Some(vqd) = extract_ddg_vqd_token(&boot_html) {
            let ddg_json_url = format!(
                "https://duckduckgo.com/i.js?l=us-en&o=json&q={}&vqd={}&f=,,,&p=1",
                encoded_q,
                urlencoding::encode(&vqd)
            );
            if let Some(json_text) = doh_fetch_text_body(&ddg_json_url, Some("https://duckduckgo.com/")).await {
                if let Ok(val) = serde_json::from_str::<Value>(&json_text) {
                    if let Some(arr) = val.get("results").and_then(|v| v.as_array()) {
                        for item in arr.iter().take(10) {
                            let w = item.get("width").and_then(|v| v.as_u64()).unwrap_or(300) as u32;
                            let h = item.get("height").and_then(|v| v.as_u64()).unwrap_or(420) as u32;
                            let aspect = w as f32 / h.max(1) as f32;
                            if w >= 200 && h >= 200 && (0.55..=1.15).contains(&aspect) {
                                if let Some(u) = item.get("image").and_then(|v| v.as_str()) {
                                    if u.starts_with("https://") {
                                        push_unique_url(&mut candidate_urls, u.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Endpoint B: Bing Images async HTML via DoH
    let bing_url = format!(
        "https://www.bing.com/images/async?q={}&first=0&count=12&adlt=moderate",
        encoded_q
    );
    if let Some(bing_html) = doh_fetch_text_body(&bing_url, Some("https://www.bing.com/")).await {
        for u in extract_ddg_image_urls_from_html(&bing_html).into_iter().take(10) {
            push_unique_url(&mut candidate_urls, u);
        }
    }

    // Endpoint C: DuckDuckGo HTML fallback
    if candidate_urls.len() < 4 {
        let ddg_html_url = format!("https://html.duckduckgo.com/html/?q={}", encoded_q);
        if let Some(html) = doh_fetch_text_body(&ddg_html_url, Some("https://duckduckgo.com/")).await {
            for u in extract_ddg_image_urls_from_html(&html).into_iter().take(6) {
                push_unique_url(&mut candidate_urls, u);
            }
        }
    }

    candidate_urls
}

async fn search_web_cover_fallback_doh(
    clean_title: &str,
    platform_hint: Option<&str>,
) -> Option<(Vec<u8>, &'static str)> {
    let candidates = collect_doh_web_image_urls(clean_title, platform_hint).await;
    for img_url in candidates.into_iter().take(10) {
        if let Some((bytes, _)) = download_verified_image_doh(
            &img_url,
            Some("https://duckduckgo.com/"),
            MIN_WEB_FALLBACK_IMAGE_BYTES,
        )
        .await
        {
            if let Some((ext, _, _)) = verify_web_cover_dimensions(&bytes) {
                return Some((bytes, ext));
            }
        }
    }
    None
}

async fn extract_exe_highres_png_bytes(exe_path: &str) -> Option<Vec<u8>> {
    let trimmed = exe_path.trim().to_string();
    if trimmed.is_empty() || !trimmed.to_ascii_lowercase().ends_with(".exe") {
        return None;
    }
    if !Path::new(&trimmed).is_file() {
        return None;
    }
    tokio::task::spawn_blocking(move || -> Option<Vec<u8>> {
        let escaped = trimmed.replace('\'', "''");
        let ps_script = format!(
            "Add-Type -AssemblyName System.Drawing; $ico = [System.Drawing.Icon]::ExtractAssociatedIcon('{}'); if ($ico) {{ $bmp = $ico.ToBitmap(); $ms = New-Object System.IO.MemoryStream; $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png); [Convert]::ToBase64String($ms.ToArray()) }}",
            escaped
        );
        let output = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps_script])
            .creation_flags(crate::CREATE_NO_WINDOW)
            .output()
            .ok()?;
        let b64 = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if b64.len() < 100 || b64.len() > 1024 * 1024 {
            return None;
        }
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
        verify_image_bytes(&bytes)?;
        Some(bytes)
    })
    .await
    .ok()?
}

async fn search_steamgriddb_grid_doh(api_key: &str, clean_title: &str) -> Option<(Vec<u8>, &'static str)> {
    if api_key.trim().is_empty() || clean_title.trim().is_empty() {
        return None;
    }
    let result = search_steamgriddb_artwork(clean_title.trim().to_string())
        .await
        .ok()?;
    for asset in result.grids.into_iter().take(4) {
        let target_url = if !asset.thumb.trim().is_empty() {
            asset.thumb.trim()
        } else {
            asset.url.trim()
        };
        if let Some(hit) = download_verified_image_doh(
            target_url,
            Some("https://www.steamgriddb.com/"),
            MIN_VALID_COVER_BYTES,
        )
        .await
        {
            return Some(hit);
        }
    }
    None
}

/// Resolves and caches a game cover into `<covers_dir>/<cache_id>.<ext>` + 32x32 PNG into `<sidebar_icons_dir>/<icon_id>.png`
/// using the 4-tier cascading pipeline: Disk Cache -> Steam Multi-CDN -> Libretro Fuzzy -> SteamGridDB / Web Search.
pub(crate) async fn resolve_and_cache_cover_in_dirs(
    req: &CoverResolveRequest,
    covers_dir: &Path,
    sidebar_icons_dir: &Path,
) -> Option<CoverResolveOutcome> {
    let _ = std::fs::create_dir_all(covers_dir);
    let _ = std::fs::create_dir_all(sidebar_icons_dir);

    let safe_cache_stem = crate::sidebar_icons::safe_sidebar_icon_stem(&req.cache_id);
    if safe_cache_stem.is_empty() {
        return None;
    }
    let icon_id = req.sidebar_icon_id.as_deref().unwrap_or(&req.cache_id);

    // 1. Local Disk Cache
    let mut cache_keys = vec![req.cache_id.clone()];
    if let Some(aid) = req.steam_app_id {
        cache_keys.push(aid.to_string());
    }
    if let Some(cached_path) = find_cached_cover_on_disk(covers_dir, &cache_keys) {
        let sidebar_icon_path = std::fs::read(&cached_path).ok().and_then(|bytes| {
            ensure_sidebar_icon_for_cover_bytes(sidebar_icons_dir, icon_id, &bytes, false)
        });
        return Some(CoverResolveOutcome {
            cover_path: cached_path,
            sidebar_icon_path,
            source_tier: "cache",
        });
    }

    // Trim freed download/decode heap buffers once the remote cascade returns (or fails).
    struct WorkingSetTrimGuard;
    impl Drop for WorkingSetTrimGuard {
        fn drop(&mut self) {
            duke_core::win32_ffi::trim_current_working_set();
        }
    }
    let _trim_guard = WorkingSetTrimGuard;

    let clean_title = clean_cover_query(&req.title);
    let mut resolved: Option<(Vec<u8>, &'static str, &'static str)> = None;

    // 2. Steam Store Search & Multi-CDN (Jaro-Winkler >= 0.78 -> library_600x900_2x / library_600x900 / header)
    let mut steam_ids = Vec::new();
    if let Some(aid) = req.steam_app_id {
        if aid > 0 {
            steam_ids.push(aid);
        }
    }
    for found_aid in search_steam_app_ids_doh(&clean_title).await {
        if !steam_ids.contains(&found_aid) {
            steam_ids.push(found_aid);
        }
    }
    'steam_loop: for aid in steam_ids.into_iter().take(2) {
        for url in build_steam_cdn_urls(aid) {
            if let Some((bytes, ext)) = download_verified_image_doh(&url, None, MIN_VALID_COVER_BYTES).await {
                resolved = Some((bytes, ext, "steam"));
                break 'steam_loop;
            }
        }
    }

    // 3. Libretro Boxarts CDN (Named_Boxarts across (USA)/(World)/(Europe)/(Japan) for all 24 consoles)
    if resolved.is_none() {
        if let Some(spec) = resolve_target_platform_spec(req.platform_id, req.platform_hint.as_deref()) {
            let candidates = build_libretro_fuzzy_candidate_urls(spec, &req.title);
            for url in candidates.into_iter().take(16) {
                if let Some((bytes, ext)) =
                    download_verified_image_doh(&url, None, MIN_VALID_COVER_BYTES).await
                {
                    resolved = Some((bytes, ext, "libretro"));
                    break;
                }
            }
        }
    }

    // 4. SteamGridDB 600x900 Vertical Grids (when API key configured)
    if resolved.is_none() && !req.steamgriddb_api_key.trim().is_empty() {
        if let Some((bytes, ext)) =
            search_steamgriddb_grid_doh(&req.steamgriddb_api_key, &clean_title).await
        {
            resolved = Some((bytes, ext, "steamgriddb"));
        }
    }

    // 5. Privacy-Routed DoH Web Image Search Fallback (>= 200x200 px, 0.55..=1.15 aspect ratio)
    if resolved.is_none() {
        if let Some((bytes, ext)) =
            search_web_cover_fallback_doh(&clean_title, req.platform_hint.as_deref()).await
        {
            resolved = Some((bytes, ext, "web"));
        }
    }
    if resolved.is_none() {
        if let Some(ref raw_existing) = req.existing_cover_url {
            let trimmed = raw_existing.trim();
            if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
                if let Some((bytes, ext)) =
                    download_verified_image_doh(trimmed, None, MIN_VALID_COVER_BYTES).await
                {
                    resolved = Some((bytes, ext, "existing_url"));
                }
            }
        }
    }

    // Stage 1b/6 Local Embedded .EXE Resource Art Fallback (if Stages 2-5 missed)
    if resolved.is_none() {
        if let Some(ref exe) = req.exe_path {
            if let Some(png_bytes) = extract_exe_highres_png_bytes(exe).await {
                resolved = Some((png_bytes, "png", "exe_icon"));
            }
        }
    }

    let (bytes, ext, source_tier) = resolved?;
    let dest_path = covers_dir.join(format!("{}.{}", safe_cache_stem, ext));
    crate::atomic_write_bytes(&dest_path, &bytes).ok()?;
    let sidebar_icon_path =
        ensure_sidebar_icon_for_cover_bytes(sidebar_icons_dir, icon_id, &bytes, true);
    Some(CoverResolveOutcome {
        cover_path: dest_path,
        sidebar_icon_path,
        source_tier,
    })
}

/// Infers retro/console `platform_id` + hint from game metadata/extension when `platform_id` isn't explicit.
pub(crate) fn infer_game_platform_hint(
    title: &str,
    exe_path: &str,
    genre: &str,
    source: &str,
) -> (Option<u16>, Option<String>) {
    let lower_exe = exe_path.trim().to_ascii_lowercase();
    let ext_pid = if lower_exe.ends_with(".nsp") || lower_exe.ends_with(".xci") {
        Some(crate::platforms::PLATFORM_SWITCH)
    } else if lower_exe.ends_with(".3ds") || lower_exe.ends_with(".cci") || lower_exe.ends_with(".cia") {
        Some(crate::platforms::PLATFORM_3DS)
    } else if lower_exe.ends_with(".nds") {
        Some(crate::platforms::PLATFORM_NDS)
    } else if lower_exe.ends_with(".gba") {
        Some(crate::platforms::PLATFORM_GBA)
    } else if lower_exe.ends_with(".gbc") {
        Some(crate::platforms::PLATFORM_GBC)
    } else if lower_exe.ends_with(".gb") {
        Some(crate::platforms::PLATFORM_GB)
    } else if lower_exe.ends_with(".z64") || lower_exe.ends_with(".n64") || lower_exe.ends_with(".v64") {
        Some(crate::platforms::PLATFORM_N64)
    } else if lower_exe.ends_with(".sfc") || lower_exe.ends_with(".smc") {
        Some(crate::platforms::PLATFORM_SNES)
    } else if lower_exe.ends_with(".nes") {
        Some(crate::platforms::PLATFORM_NES)
    } else if lower_exe.ends_with(".rvz") || lower_exe.ends_with(".gcm") {
        Some(crate::platforms::PLATFORM_GAMECUBE)
    } else if lower_exe.ends_with(".wbfs") || lower_exe.ends_with(".wad") {
        Some(crate::platforms::PLATFORM_WII)
    } else {
        None
    };
    if let Some(pid) = ext_pid {
        let spec = crate::platforms::platform_by_id(pid);
        return (Some(pid), Some(spec.short_name.to_string()));
    }

    for candidate in [source, genre, title] {
        if let Some(spec) = resolve_target_platform_spec(None, Some(candidate)) {
            return (Some(spec.id), Some(spec.short_name.to_string()));
        }
    }
    (None, Some("PC".to_string()))
}

#[tauri::command]
pub(crate) async fn resolve_catalog_cover(
    app: AppHandle,
    app_id: u32,
    title: String,
    platform_id: Option<u16>,
    platform_name: Option<String>,
    cover_url: Option<String>,
) -> Result<Option<String>, String> {
    let covers_dir = crate::get_covers_dir(&app)?;
    let sidebar_icons_dir = crate::sidebar_icons::get_sidebar_icons_dir(&app)?;
    let is_retro = platform_id.map(|p| p != crate::platforms::PLATFORM_PC).unwrap_or(false)
        || platform_name
            .as_deref()
            .and_then(|h| resolve_target_platform_spec(None, Some(h)))
            .is_some();
    let cache_id = if is_retro {
        format!("retro_{}", app_id)
    } else {
        app_id.to_string()
    };
    let settings = crate::load_settings_from_disk(&app);
    let req = CoverResolveRequest {
        cache_id: cache_id.clone(),
        title,
        exe_path: None,
        platform_id,
        platform_hint: platform_name,
        steam_app_id: if !is_retro && app_id > 0 && app_id < 0x8000_0000 {
            Some(app_id)
        } else {
            None
        },
        existing_cover_url: cover_url,
        steamgriddb_api_key: settings.steamgriddb_api_key.unwrap_or_default(),
        sidebar_icon_id: Some(cache_id),
    };
    if let Some(outcome) =
        resolve_and_cache_cover_in_dirs(&req, &covers_dir, &sidebar_icons_dir).await
    {
        let cover_path_str = outcome.cover_path.to_string_lossy().to_string();
        if outcome.source_tier != "cache" {
            crate::emit_if_alive(
                &app,
                "cover-updated",
                serde_json::json!({
                    "gameId": req.cache_id,
                    "coverUrl": &cover_path_str,
                }),
            );
        }
        return Ok(Some(cover_path_str));
    }
    Ok(None)
}

#[tauri::command]
pub(crate) async fn search_web_covers_for_game(
    query: String,
    platform_hint: Option<String>,
    steam_app_id: Option<u32>,
) -> Result<Vec<WebCoverCandidate>, String> {
    let clean = clean_cover_query(&query);
    if clean.trim().is_empty() && steam_app_id.unwrap_or(0) == 0 {
        return Ok(Vec::new());
    }
    let mut out: Vec<WebCoverCandidate> = Vec::new();
    let mut push_candidate = |url: String, source: &str, label: &str| {
        if !url.starts_with("https://") && !url.starts_with("http://") {
            return;
        }
        if out.iter().any(|c| c.url == url) {
            return;
        }
        out.push(WebCoverCandidate {
            thumb: url.clone(),
            url,
            source: source.to_string(),
            label: label.to_string(),
        });
    };

    if let Some(aid) = steam_app_id.filter(|&v| v > 0) {
        let urls = build_steam_cdn_urls(aid);
        push_candidate(urls[0].clone(), "Steam", &format!("{clean} (600x900)"));
        push_candidate(urls[2].clone(), "Steam", &format!("{clean} (Header)"));
    }

    for aid in search_steam_app_ids_doh(&clean).await.into_iter().take(4) {
        let urls = build_steam_cdn_urls(aid);
        push_candidate(urls[0].clone(), "Steam", &format!("Steam #{aid} (600x900)"));
        push_candidate(urls[2].clone(), "Steam", &format!("Steam #{aid} (Header)"));
    }

    if let Some(spec) = resolve_target_platform_spec(None, platform_hint.as_deref()) {
        for url in build_libretro_fuzzy_candidate_urls(spec, &query).into_iter().take(4) {
            push_candidate(url, "Libretro", spec.short_name);
        }
    }

    for url in collect_doh_web_image_urls(&clean, platform_hint.as_deref())
        .await
        .into_iter()
        .take(12)
    {
        push_candidate(url, "Web Image", &clean);
    }

    duke_core::win32_ffi::trim_current_working_set();
    Ok(out.into_iter().take(18).collect())
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

    #[test]
    fn cascading_cover_helpers_strip_tags_validate_images_and_cache() {
        assert_eq!(
            strip_region_revision_tags("The Legend of Zelda: Ocarina of Time (USA) (Rev 2) [!]"),
            "The Legend of Zelda: Ocarina of Time"
        );
        assert!(titles_match_for_cover(
            "Hollow Knight (USA)",
            "Hollow Knight"
        ));
        assert!(!titles_match_for_cover("Super Mario 64", "Hollow Knight"));

        let switch_spec = crate::platforms::platform_by_id(crate::platforms::PLATFORM_SWITCH);
        let urls = build_libretro_fuzzy_candidate_urls(
            switch_spec,
            "Mario & Luigi: Brothership (USA)",
        );
        assert!(urls
            .first()
            .is_some_and(|u| u.contains("/Named_Boxarts/Mario%20_%20Luigi_%20Brothership")));
        assert!(urls.iter().any(|u| u.contains("/Named_Snaps/")));
        assert!(urls.iter().any(|u| u.contains("/Named_Titles/")));

        let html = r#"
            <img src="https://duckduckgo.com/assets/logo.png"/>
            <a href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fcdn.example.org%2Fcovers%2Fzelda_box.jpg">Box</a>
            <img src="https://images.example.net/switch/metroid.webp?w=600"/>
        "#;
        let extracted = extract_ddg_image_urls_from_html(html);
        assert_eq!(
            extracted,
            vec![
                "https://cdn.example.org/covers/zelda_box.jpg".to_string(),
                "https://images.example.net/switch/metroid.webp?w=600".to_string(),
            ]
        );

        assert!(verify_image_bytes(b"<html>404 Not Found</html>").is_none());

        let mut rgba = vec![0xFFu8; 64 * 64 * 4];
        for (i, b) in rgba.iter_mut().enumerate() {
            if i % 4 != 3 {
                *b = ((i.wrapping_mul(73) ^ (i >> 3)) & 0xFF) as u8;
            }
        }
        let png_bytes = crate::sidebar_icons::encode_rgba_png(&rgba, 64, 64).unwrap();
        assert_eq!(verify_image_bytes(&png_bytes).map(|x| x.0), Some("png"));

        let tmp = std::env::temp_dir().join(format!("duke_cover_cache_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let covers_dir = tmp.join("covers");
        let icons_dir = tmp.join("sidebar_icons");
        std::fs::create_dir_all(&covers_dir).unwrap();
        std::fs::write(covers_dir.join("steam_250820.png"), &png_bytes).unwrap();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let outcome = rt.block_on(resolve_and_cache_cover_in_dirs(
            &CoverResolveRequest {
                cache_id: "steam_250820".to_string(),
                title: "SteamVR".to_string(),
                exe_path: None,
                platform_id: None,
                platform_hint: Some("PC".to_string()),
                steam_app_id: Some(250820),
                existing_cover_url: None,
                steamgriddb_api_key: String::new(),
                sidebar_icon_id: Some("steam_250820".to_string()),
            },
            &covers_dir,
            &icons_dir,
        ));
        let out = outcome.expect("cached cover should resolve immediately");
        assert_eq!(out.source_tier, "cache");
        assert!(out.cover_path.exists());
        assert!(icons_dir.join("steam_250820.png").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn clean_cover_query_jaro_winkler_and_aspect_ratio_checks() {
        assert_eq!(clean_cover_query("Feed-and-Grow-Fish-Steam..."), "Feed and Grow Fish");
        assert_eq!(
            clean_cover_query("Hollow_Knight_v1.5.78_(USA)_[!]_GOTY_Edition"),
            "Hollow Knight"
        );
        assert_eq!(
            clean_cover_query("Elden Ring (Europe) (Disc 1) v1.12.2 [FitGirl Repack]"),
            "Elden Ring"
        );

        assert!(jaro_winkler_similarity("Feed and Grow Fish", "Feed & Grow: Fish") >= 0.78);
        assert!(jaro_winkler_similarity("Super Mario 64", "Hollow Knight") < 0.78);

        // Reject too small (< 200x200)
        let small_png = crate::sidebar_icons::encode_rgba_png(&vec![0x80; 100 * 150 * 4], 100, 150).unwrap();
        assert!(verify_web_cover_dimensions(&small_png).is_none());

        // Reject widescreen banner (600x200 -> ratio 3.0 outside 0.55..=1.15)
        let banner_png = crate::sidebar_icons::encode_rgba_png(&vec![0x80; 600 * 200 * 4], 600, 200).unwrap();
        assert!(verify_web_cover_dimensions(&banner_png).is_none());

        // Accept valid box-art dimensions (300x450 -> ratio 0.667 inside 0.55..=1.15)
        let box_png = crate::sidebar_icons::encode_rgba_png(&vec![0x80; 300 * 450 * 4], 300, 450).unwrap();
        assert_eq!(verify_web_cover_dimensions(&box_png), Some(("png", 300, 450)));

        let bing_html = r#"<a m="{"murl":"https://img.example.org/boxart/metroid_prime.jpg"}"></a>"#;
        let bing_html_esc = r#"&quot;murl&quot;:&quot;https://img.example.org/boxart/metroid_prime.jpg&quot;"#;
        let found = extract_ddg_image_urls_from_html(&format!("{bing_html} {bing_html_esc}"));
        assert!(found.iter().any(|u| u.ends_with("metroid_prime.jpg")));
    }
}
