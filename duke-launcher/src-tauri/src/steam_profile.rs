//! Zero-Account Steam Public Profile & Cosmetics Resolver (`v2.3.0`).
//!
//! Resolves the local user's (or any previewed `SteamID64` / vanity URL's) Steam Mini-Profile:
//! - Auto-detects local `SteamID64` and persona fallback from `Steam\config\loginusers.vdf`.
//! - Fetches zero-key public XML feed (`https://steamcommunity.com/profiles/<id>/?xml=1` or
//!   `/id/<vanity>/?xml=1`) over the pooled `net_engine` HTTP/2 + DoH client.
//! - Optionally enriches with Steam Level, equipped animated avatar, avatar frame, profile
//!   background, and live `gameextrainfo` when a DPAPI-encrypted Steam Web API key is present.
//! - Caches responses for 10 minutes (`600s` TTL) in memory and on disk (`steam_profile_cache.json`)
//!   and degrades gracefully when offline or rate-limited.

use reqwest::header::ACCEPT;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const STEAM_PROFILE_CACHE_TTL_SECS: u64 = 10 * 60;
const STEAM_PROFILE_CACHE_FILE: &str = "steam_profile_cache.json";
const STEAM_CDN_IMAGES_BASE: &str =
    "https://cdn.cloudflare.steamstatic.com/steamcommunity/public/images/";

static PROFILE_MEM_CACHE: OnceLock<Mutex<HashMap<String, SteamMiniProfile>>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SteamProfileTarget {
    SteamId64(String),
    VanityId(String),
}

impl SteamProfileTarget {
    pub fn cache_key(&self) -> String {
        match self {
            Self::SteamId64(id) => format!("id64:{id}"),
            Self::VanityId(v) => format!("vanity:{}", v.to_ascii_lowercase()),
        }
    }

    pub fn xml_url(&self) -> String {
        match self {
            Self::SteamId64(id) => {
                format!(
                    "https://steamcommunity.com/profiles/{}/?xml=1",
                    urlencoding::encode(id)
                )
            }
            Self::VanityId(vanity) => {
                format!(
                    "https://steamcommunity.com/id/{}/?xml=1",
                    urlencoding::encode(vanity)
                )
            }
        }
    }

    pub fn web_profile_url(&self) -> String {
        match self {
            Self::SteamId64(id) => format!("https://steamcommunity.com/profiles/{id}"),
            Self::VanityId(vanity) => format!("https://steamcommunity.com/id/{vanity}"),
        }
    }

    pub fn display_input(&self) -> String {
        match self {
            Self::SteamId64(id) => id.clone(),
            Self::VanityId(v) => v.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct SteamShowcaseGame {
    pub app_id: Option<u32>,
    pub title: String,
    pub icon_url: String,
    pub header_url: String,
    pub hours_two_weeks: String,
    pub hours_total: String,
    pub store_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct EquippedSteamCosmetics {
    pub animated_avatar_url: Option<String>,
    pub avatar_frame_url: Option<String>,
    pub avatar_frame_title: Option<String>,
    pub profile_background_url: Option<String>,
    pub profile_background_title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SteamMiniProfile {
    pub configured: bool,
    pub target_input: String,
    pub steam_id64: String,
    pub vanity_id: Option<String>,
    pub is_override_preview: bool,
    pub detected_local_steam_id: Option<String>,
    pub persona_name: String,
    pub real_name: Option<String>,
    pub avatar_url: String,
    pub animated_avatar_url: Option<String>,
    pub avatar_frame_url: Option<String>,
    pub avatar_frame_title: Option<String>,
    pub profile_background_url: Option<String>,
    pub profile_background_title: Option<String>,
    pub online_state: String,
    pub status_label: String,
    pub in_game_title: Option<String>,
    pub in_game_app_id: Option<u32>,
    pub summary_text: String,
    pub member_since: Option<String>,
    pub location: Option<String>,
    pub steam_level: Option<u32>,
    pub privacy_state: String,
    pub stranger_visibility_label: String,
    pub profile_url: String,
    pub showcase_games: Vec<SteamShowcaseGame>,
    pub enriched_via_api_key: bool,
    pub from_cache: bool,
    pub offline_fallback: bool,
    pub status_note: Option<String>,
    pub fetched_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct SteamProfileDiskCache {
    pub entries: HashMap<String, SteamMiniProfile>,
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn mem_cache() -> &'static Mutex<HashMap<String, SteamMiniProfile>> {
    PROFILE_MEM_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn is_valid_steam_id64(raw: &str) -> bool {
    let trimmed = raw.trim();
    trimmed.len() == 17
        && trimmed.bytes().all(|b| b.is_ascii_digit())
        && trimmed.starts_with("7656119")
}

/// Parses a `SteamID64`, `steamcommunity.com/profiles/<id>`, `steamcommunity.com/id/<vanity>`,
/// or bare vanity slug into a structured `SteamProfileTarget`.
pub fn parse_steam_profile_target(raw: &str) -> Option<SteamProfileTarget> {
    let trimmed = raw.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return None;
    }

    if is_valid_steam_id64(trimmed) {
        return Some(SteamProfileTarget::SteamId64(trimmed.to_string()));
    }

    let without_query = trimmed
        .split('?')
        .next()
        .unwrap_or(trimmed)
        .split('#')
        .next()
        .unwrap_or(trimmed)
        .trim_end_matches('/');

    let lower = without_query.to_ascii_lowercase();
    if let Some(idx) = lower.find("/profiles/") {
        let after = &without_query[idx + "/profiles/".len()..];
        let seg = after.split('/').next().unwrap_or("").trim();
        if is_valid_steam_id64(seg) {
            return Some(SteamProfileTarget::SteamId64(seg.to_string()));
        }
    }

    if let Some(idx) = lower.find("/id/") {
        let after = &without_query[idx + "/id/".len()..];
        let seg = after.split('/').next().unwrap_or("").trim();
        if is_valid_vanity_slug(seg) {
            return Some(SteamProfileTarget::VanityId(seg.to_string()));
        }
    }

    if is_valid_vanity_slug(without_query) {
        return Some(SteamProfileTarget::VanityId(without_query.to_string()));
    }

    None
}

fn is_valid_vanity_slug(slug: &str) -> bool {
    let s = slug.trim();
    if s.len() < 2 || s.len() > 64 {
        return false;
    }
    s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Extracts `(SteamID64, PersonaName)` for the most recent user in `Steam/config/loginusers.vdf`.
pub fn parse_local_user_from_loginusers_vdf(vdf_text: &str) -> Option<(String, Option<String>)> {
    let tree = crate::importers::parse_vdf_kv_tree(vdf_text);
    let users = tree.get_node("users").or(Some(&tree)).and_then(|n| n.as_map())?;

    let mut best: Option<(bool, u64, String, Option<String>)> = None;
    for (steam_id, node) in users {
        let candidate_id = steam_id.trim();
        if !is_valid_steam_id64(candidate_id) {
            continue;
        }
        let most_recent = node
            .get_str("MostRecent")
            .or_else(|| node.get_str("mostrecent"))
            .map(|v| v.trim() == "1" || v.trim().eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let timestamp = node
            .get_str("Timestamp")
            .or_else(|| node.get_str("timestamp"))
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(0);
        let persona = node
            .get_str("PersonaName")
            .or_else(|| node.get_str("personaname"))
            .or_else(|| node.get_str("AccountName"))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let replace = match &best {
            None => true,
            Some((prev_recent, prev_ts, _, _)) => {
                (most_recent, timestamp) > (*prev_recent, *prev_ts)
            }
        };
        if replace {
            best = Some((
                most_recent,
                timestamp,
                candidate_id.to_string(),
                persona,
            ));
        }
    }

    best.map(|(_, _, id, persona)| (id, persona))
}

pub fn detect_local_steam_user() -> Option<(String, Option<String>)> {
    let mut roots = Vec::new();
    if let Some(reg_path) =
        duke_core::win32_ffi::reg_read_string(false, "Software\\Valve\\Steam", "SteamPath")
            .or_else(|| {
                duke_core::win32_ffi::reg_read_string(
                    true,
                    "SOFTWARE\\WOW6432Node\\Valve\\Steam",
                    "InstallPath",
                )
            })
            .or_else(|| {
                duke_core::win32_ffi::reg_read_string(
                    true,
                    "SOFTWARE\\Valve\\Steam",
                    "InstallPath",
                )
            })
    {
        let p = std::path::PathBuf::from(reg_path.replace('/', "\\"));
        if p.is_dir() {
            roots.push(p);
        }
    }
    for env_key in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
        if let Ok(base) = std::env::var(env_key) {
            let cand = std::path::PathBuf::from(base).join("Steam");
            if cand.is_dir() {
                roots.push(cand);
            }
        }
    }
    for root in roots {
        let vdf_path = root.join("config").join("loginusers.vdf");
        if let Ok(raw) = fs::read_to_string(&vdf_path) {
            if let Some(found) = parse_local_user_from_loginusers_vdf(&raw) {
                return Some(found);
            }
        }
    }
    None
}

fn extract_xml_tag_raw<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let rest = &xml[start..];
    let end = rest.find(&close)?;
    Some(&rest[..end])
}

fn clean_xml_value(raw: &str) -> String {
    let trimmed = raw.trim();
    let inner = if let Some(stripped) = trimmed
        .strip_prefix("<![CDATA[")
        .and_then(|s| s.strip_suffix("]]>"))
    {
        stripped.trim()
    } else {
        trimmed
    };
    decode_html_entities(inner)
}

fn decode_html_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

pub fn strip_html_to_plain_text(raw: &str) -> String {
    let with_breaks = raw
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("<br>", "\n")
        .replace("<BR>", "\n")
        .replace("</p>", "\n");
    let mut out = String::with_capacity(with_breaks.len());
    let mut in_tag = false;
    for ch in with_breaks.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    let decoded = decode_html_entities(&out);
    let lines: Vec<String> = decoded
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(ToString::to_string)
        .collect();
    let joined = lines.join("\n");
    if joined.chars().count() > 280 {
        let truncated: String = joined.chars().take(277).collect();
        format!("{truncated}...")
    } else {
        joined
    }
}

pub fn extract_app_id_from_steam_link(link: &str) -> Option<u32> {
    let lower = link.trim().to_ascii_lowercase();
    for marker in ["/app/", "/apps/"] {
        if let Some(idx) = lower.find(marker) {
            let rest = &lower[idx + marker.len()..];
            let digits: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(app_id) = digits.parse::<u32>() {
                if app_id > 0 {
                    return Some(app_id);
                }
            }
        }
    }
    None
}

pub fn format_stranger_visibility_label(privacy_state: &str) -> (&'static str, &'static str) {
    match privacy_state.trim().to_ascii_lowercase().as_str() {
        "public" | "3" => ("public", "Public Profile — Visible to Strangers"),
        "friendsonly" | "2" => (
            "friendsonly",
            "Friends-Only — Strangers See Basic Avatar Only",
        ),
        "private" | "1" => ("private", "Private Profile — Hidden From Strangers"),
        _ => ("public", "Public Profile — Visible to Strangers"),
    }
}

/// Parses Valve's zero-key `?xml=1` community profile response into a `SteamMiniProfile`.
pub fn parse_steam_community_xml(
    xml: &str,
    fallback_target: &SteamProfileTarget,
) -> Result<SteamMiniProfile, String> {
    if let Some(err_msg) = extract_xml_tag_raw(xml, "error").map(clean_xml_value) {
        if !err_msg.is_empty() {
            return Err(format!("Steam Community reported: {err_msg}"));
        }
    }

    let steam_id64 = extract_xml_tag_raw(xml, "steamID64")
        .map(clean_xml_value)
        .filter(|s| is_valid_steam_id64(s))
        .unwrap_or_else(|| match fallback_target {
            SteamProfileTarget::SteamId64(id) => id.clone(),
            SteamProfileTarget::VanityId(_) => String::new(),
        });

    let persona_name = extract_xml_tag_raw(xml, "steamID")
        .map(clean_xml_value)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if !steam_id64.is_empty() {
                format!("Steam User {}", &steam_id64[steam_id64.len().saturating_sub(4)..])
            } else {
                fallback_target.display_input()
            }
        });

    if steam_id64.is_empty() && extract_xml_tag_raw(xml, "steamID").is_none() {
        return Err("Invalid or unrecognized Steam Community XML feed".to_string());
    }

    let raw_privacy = extract_xml_tag_raw(xml, "privacyState")
        .map(clean_xml_value)
        .unwrap_or_else(|| "public".to_string());
    let (privacy_state, visibility_label) = format_stranger_visibility_label(&raw_privacy);

    let avatar_url = extract_xml_tag_raw(xml, "avatarFull")
        .or_else(|| extract_xml_tag_raw(xml, "avatarMedium"))
        .or_else(|| extract_xml_tag_raw(xml, "avatarIcon"))
        .map(clean_xml_value)
        .unwrap_or_default();

    let vanity_id = extract_xml_tag_raw(xml, "customURL")
        .map(clean_xml_value)
        .filter(|s| !s.is_empty());

    let raw_online = extract_xml_tag_raw(xml, "onlineState")
        .map(clean_xml_value)
        .unwrap_or_else(|| "offline".to_string())
        .to_ascii_lowercase();

    let state_msg = extract_xml_tag_raw(xml, "stateMessage")
        .map(clean_xml_value)
        .map(|s| strip_html_to_plain_text(&s))
        .unwrap_or_default();

    let mut in_game_title = None;
    let mut in_game_app_id = None;
    if let Some(ig_block) = extract_xml_tag_raw(xml, "inGameInfo") {
        in_game_title = extract_xml_tag_raw(ig_block, "gameName")
            .map(clean_xml_value)
            .filter(|s| !s.is_empty());
        if let Some(link) = extract_xml_tag_raw(ig_block, "gameLink").map(clean_xml_value) {
            in_game_app_id = extract_app_id_from_steam_link(&link);
        }
    }

    let (online_state, status_label) = if raw_online.contains("in-game") || in_game_title.is_some() {
        let game = in_game_title
            .clone()
            .unwrap_or_else(|| state_msg.replace("In-Game", "").trim().to_string());
        let label = if game.is_empty() {
            "In-Game".to_string()
        } else {
            format!("In-Game · {game}")
        };
        ("in-game".to_string(), label)
    } else if raw_online.contains("online") {
        let label = if !state_msg.is_empty() && !state_msg.eq_ignore_ascii_case("online") {
            state_msg
        } else {
            "Online".to_string()
        };
        ("online".to_string(), label)
    } else {
        let label = if !state_msg.is_empty() {
            state_msg
        } else {
            "Offline".to_string()
        };
        ("offline".to_string(), label)
    };

    let summary_text = extract_xml_tag_raw(xml, "summary")
        .map(clean_xml_value)
        .map(|s| strip_html_to_plain_text(&s))
        .filter(|s| !s.eq_ignore_ascii_case("No information given."))
        .unwrap_or_default();

    let member_since = extract_xml_tag_raw(xml, "memberSince")
        .map(clean_xml_value)
        .filter(|s| !s.is_empty());

    let location = extract_xml_tag_raw(xml, "location")
        .map(clean_xml_value)
        .filter(|s| !s.is_empty());

    let real_name = extract_xml_tag_raw(xml, "realname")
        .map(clean_xml_value)
        .filter(|s| !s.is_empty());

    let mut showcase_games = Vec::new();
    if let Some(mp_section) = extract_xml_tag_raw(xml, "mostPlayedGames") {
        let mut cursor = mp_section;
        while let Some(open_idx) = cursor.find("<mostPlayedGame>") {
            let after_open = &cursor[open_idx + "<mostPlayedGame>".len()..];
            let Some(close_idx) = after_open.find("</mostPlayedGame>") else {
                break;
            };
            let item_xml = &after_open[..close_idx];
            cursor = &after_open[close_idx + "</mostPlayedGame>".len()..];

            let title = extract_xml_tag_raw(item_xml, "gameName")
                .map(clean_xml_value)
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let game_link = extract_xml_tag_raw(item_xml, "gameLink")
                .map(clean_xml_value)
                .unwrap_or_default();
            let app_id = extract_app_id_from_steam_link(&game_link);
            let icon_url = extract_xml_tag_raw(item_xml, "gameIcon")
                .or_else(|| extract_xml_tag_raw(item_xml, "gameLogoSmall"))
                .map(clean_xml_value)
                .unwrap_or_default();
            let logo_url = extract_xml_tag_raw(item_xml, "gameLogo")
                .map(clean_xml_value)
                .unwrap_or_default();
            let header_url = if let Some(aid) = app_id {
                format!(
                    "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{aid}/header.jpg"
                )
            } else if !logo_url.is_empty() {
                logo_url
            } else {
                icon_url.clone()
            };
            let hours_two_weeks = extract_xml_tag_raw(item_xml, "hoursPlayed")
                .map(clean_xml_value)
                .unwrap_or_else(|| "0".to_string());
            let hours_total = extract_xml_tag_raw(item_xml, "hoursOnRecord")
                .map(clean_xml_value)
                .unwrap_or_else(|| hours_two_weeks.clone());
            let store_url = if let Some(aid) = app_id {
                format!("https://store.steampowered.com/app/{aid}")
            } else if !game_link.is_empty() {
                game_link
            } else {
                "https://store.steampowered.com".to_string()
            };

            showcase_games.push(SteamShowcaseGame {
                app_id,
                title,
                icon_url,
                header_url,
                hours_two_weeks,
                hours_total,
                store_url,
            });
            if showcase_games.len() >= 3 {
                break;
            }
        }
    }

    let profile_url = if !steam_id64.is_empty() {
        format!("https://steamcommunity.com/profiles/{steam_id64}")
    } else {
        fallback_target.web_profile_url()
    };

    Ok(SteamMiniProfile {
        configured: true,
        target_input: fallback_target.display_input(),
        steam_id64,
        vanity_id,
        is_override_preview: false,
        detected_local_steam_id: None,
        persona_name,
        real_name,
        avatar_url,
        animated_avatar_url: None,
        avatar_frame_url: None,
        avatar_frame_title: None,
        profile_background_url: None,
        profile_background_title: None,
        online_state,
        status_label,
        in_game_title,
        in_game_app_id,
        summary_text,
        member_since,
        location,
        steam_level: None,
        privacy_state: privacy_state.to_string(),
        stranger_visibility_label: visibility_label.to_string(),
        profile_url,
        showcase_games,
        enriched_via_api_key: false,
        from_cache: false,
        offline_fallback: false,
        status_note: None,
        fetched_at: now_unix_secs(),
    })
}

fn qualify_steam_economy_asset(raw_path: &str) -> Option<String> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
        return Some(trimmed.to_string());
    }
    let clean = trimmed.trim_start_matches('/');
    Some(format!("{STEAM_CDN_IMAGES_BASE}{clean}"))
}

/// Extracts equipped animated avatar, avatar frame, and profile background from
/// `IPlayerService/GetProfileItemsEquipped/v1` (or individual `GetAnimatedAvatar/v1`,
/// `GetAvatarFrame/v1`, `GetProfileBackground/v1` JSON payloads).
pub fn parse_equipped_profile_items_json(root: &Value) -> EquippedSteamCosmetics {
    let resp = root.get("response").unwrap_or(root);
    let mut out = EquippedSteamCosmetics::default();

    if let Some(anim) = resp.get("animated_avatar") {
        let asset = anim
            .get("image_small")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .or_else(|| anim.get("image_large").and_then(|v| v.as_str()));
        if let Some(p) = asset.and_then(qualify_steam_economy_asset) {
            out.animated_avatar_url = Some(p);
        }
    }

    if let Some(frame) = resp.get("avatar_frame") {
        let asset = frame
            .get("image_small")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .or_else(|| frame.get("image_large").and_then(|v| v.as_str()));
        if let Some(p) = asset.and_then(qualify_steam_economy_asset) {
            out.avatar_frame_url = Some(p);
        }
        out.avatar_frame_title = frame
            .get("item_title")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
    }

    if let Some(bg) = resp
        .get("profile_background")
        .filter(|v| v.is_object() && !v.as_object().map(|o| o.is_empty()).unwrap_or(true))
        .or_else(|| resp.get("mini_profile_background"))
    {
        let asset = bg
            .get("image_large")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .or_else(|| bg.get("movie_webm").and_then(|v| v.as_str()))
            .or_else(|| bg.get("movie_mp4").and_then(|v| v.as_str()));
        if let Some(p) = asset.and_then(qualify_steam_economy_asset) {
            out.profile_background_url = Some(p);
        }
        out.profile_background_title = bg
            .get("item_title")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
    }

    out
}

async fn fetch_pooled_text(url: &str, accept: &str) -> Result<String, String> {
    let client =
        crate::net_engine::get_pooled_client_for_url(url, Some(Duration::from_secs(8)), true).await;
    let resp = client
        .get(url)
        .header(ACCEPT, accept)
        .send()
        .await
        .map_err(|_| "Steam Community network request failed".to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Steam Community HTTP {}", resp.status().as_u16()));
    }
    resp.text()
        .await
        .map_err(|_| "Failed reading Steam Community response".to_string())
}

async fn fetch_pooled_json(url: &str) -> Result<Value, String> {
    let client =
        crate::net_engine::get_pooled_client_for_url(url, Some(Duration::from_secs(7)), true).await;
    let resp = client
        .get(url)
        .header(ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| "Steam Web API network request failed".to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Steam Web API HTTP {}", resp.status().as_u16()));
    }
    resp.json::<Value>()
        .await
        .map_err(|_| "Invalid Steam Web API JSON".to_string())
}

async fn enrich_profile_with_web_api(profile: &mut SteamMiniProfile, api_key: &str) {
    if !is_valid_steam_id64(&profile.steam_id64) || api_key.trim().is_empty() {
        return;
    }
    let key_enc = urlencoding::encode(api_key.trim());
    let sid_enc = urlencoding::encode(profile.steam_id64.trim());

    let summaries_url = format!(
        "https://api.steampowered.com/ISteamUser/GetPlayerSummaries/v0002/?key={key_enc}&steamids={sid_enc}"
    );
    let level_url = format!(
        "https://api.steampowered.com/IPlayerService/GetSteamLevel/v1/?key={key_enc}&steamid={sid_enc}"
    );
    let equipped_url = format!(
        "https://api.steampowered.com/IPlayerService/GetProfileItemsEquipped/v1/?key={key_enc}&steamid={sid_enc}"
    );

    let (sum_res, lvl_res, eq_res) = tokio::join!(
        fetch_pooled_json(&summaries_url),
        fetch_pooled_json(&level_url),
        fetch_pooled_json(&equipped_url),
    );

    let mut any_enriched = false;

    if let Ok(sum_val) = sum_res {
        if let Some(player) = sum_val
            .get("response")
            .and_then(|r| r.get("players"))
            .and_then(|p| p.as_array())
            .and_then(|arr| arr.first())
        {
            any_enriched = true;
            if let Some(name) = player
                .get("personaname")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                profile.persona_name = name.to_string();
            }
            if let Some(av) = player
                .get("avatarfull")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                profile.avatar_url = av.to_string();
            }
            if let Some(vis) = player.get("communityvisibilitystate").and_then(|v| v.as_u64()) {
                let code = vis.to_string();
                let (state, label) = format_stranger_visibility_label(&code);
                profile.privacy_state = state.to_string();
                profile.stranger_visibility_label = label.to_string();
            }
            let game_extra = player
                .get("gameextrainfo")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string);
            let game_id = player
                .get("gameid")
                .and_then(|v| v.as_str())
                .and_then(|s| s.trim().parse::<u32>().ok());
            if let Some(gtitle) = game_extra {
                profile.in_game_title = Some(gtitle.clone());
                profile.in_game_app_id = game_id.or(profile.in_game_app_id);
                profile.online_state = "in-game".to_string();
                profile.status_label = format!("In-Game · {gtitle}");
            } else if let Some(pstate) = player.get("personastate").and_then(|v| v.as_u64()) {
                if pstate == 0 {
                    profile.online_state = "offline".to_string();
                    profile.status_label = "Offline".to_string();
                } else {
                    profile.online_state = "online".to_string();
                    profile.status_label = match pstate {
                        2 => "Busy".to_string(),
                        3 => "Away".to_string(),
                        4 => "Snooze".to_string(),
                        _ => "Online".to_string(),
                    };
                }
            }
        }
    }

    if let Ok(lvl_val) = lvl_res {
        if let Some(lvl) = lvl_val
            .get("response")
            .and_then(|r| r.get("player_level"))
            .and_then(|v| v.as_u64())
        {
            profile.steam_level = Some(lvl as u32);
            any_enriched = true;
        }
    }

    if let Ok(eq_val) = eq_res {
        let cosmetics = parse_equipped_profile_items_json(&eq_val);
        if cosmetics.animated_avatar_url.is_some()
            || cosmetics.avatar_frame_url.is_some()
            || cosmetics.profile_background_url.is_some()
        {
            any_enriched = true;
        }
        profile.animated_avatar_url = cosmetics.animated_avatar_url;
        profile.avatar_frame_url = cosmetics.avatar_frame_url;
        profile.avatar_frame_title = cosmetics.avatar_frame_title;
        profile.profile_background_url = cosmetics.profile_background_url;
        profile.profile_background_title = cosmetics.profile_background_title;
    }

    profile.enriched_via_api_key = any_enriched;
}

fn fill_local_library_showcase_fallback(profile: &mut SteamMiniProfile) {
    if !profile.showcase_games.is_empty() {
        return;
    }
    let Ok(mut lib) = crate::load_library_standalone() else {
        return;
    };
    lib.sort_by_key(|b| std::cmp::Reverse(b.total_playtime_secs()));
    for entry in lib.into_iter().filter(|g| g.total_playtime_secs() > 0).take(3) {
        let app_id = entry
            .app_id
            .or_else(|| entry.steam_app_id.and_then(|v| u32::try_from(v).ok()));
        let hours = (entry.total_playtime_secs() as f64) / 3600.0;
        let hours_fmt = if hours >= 10.0 {
            format!("{hours:.0}")
        } else {
            format!("{hours:.1}")
        };
        let header_url = if let Some(aid) = app_id {
            format!(
                "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{aid}/header.jpg"
            )
        } else {
            entry.cover_url.clone()
        };
        let store_url = if let Some(aid) = app_id {
            format!("https://store.steampowered.com/app/{aid}")
        } else {
            "https://store.steampowered.com".to_string()
        };
        profile.showcase_games.push(SteamShowcaseGame {
            app_id,
            title: entry.title,
            icon_url: entry.cover_url,
            header_url,
            hours_two_weeks: "--".to_string(),
            hours_total: hours_fmt,
            store_url,
        });
    }
}

fn load_disk_cache() -> SteamProfileDiskCache {
    let Ok(path) = crate::get_app_data_file_standalone(STEAM_PROFILE_CACHE_FILE) else {
        return SteamProfileDiskCache::default();
    };
    let Ok(raw) = fs::read_to_string(&path) else {
        return SteamProfileDiskCache::default();
    };
    serde_json::from_str::<SteamProfileDiskCache>(&raw).unwrap_or_default()
}

fn save_disk_cache_entry(key: &str, profile: &SteamMiniProfile) {
    let Ok(path) = crate::get_app_data_file_standalone(STEAM_PROFILE_CACHE_FILE) else {
        return;
    };
    let mut disk = load_disk_cache();
    if disk.entries.len() > 24 {
        disk.entries.clear();
    }
    disk.entries.insert(key.to_string(), profile.clone());
    if let Ok(serialized) = serde_json::to_string_pretty(&disk) {
        let _ = crate::atomic_write_string(&path, &serialized);
    }
}

fn lookup_cached_profile(key: &str, allow_stale: bool) -> Option<SteamMiniProfile> {
    let now = now_unix_secs();
    if let Ok(guard) = mem_cache().lock() {
        if let Some(hit) = guard.get(key) {
            if allow_stale
                || now.saturating_sub(hit.fetched_at) <= STEAM_PROFILE_CACHE_TTL_SECS
            {
                let mut cloned = hit.clone();
                cloned.from_cache = true;
                return Some(cloned);
            }
        }
    }
    let disk = load_disk_cache();
    if let Some(hit) = disk.entries.get(key) {
        if allow_stale || now.saturating_sub(hit.fetched_at) <= STEAM_PROFILE_CACHE_TTL_SECS {
            let mut cloned = hit.clone();
            cloned.from_cache = true;
            if let Ok(mut guard) = mem_cache().lock() {
                guard.insert(key.to_string(), hit.clone());
            }
            return Some(cloned);
        }
    }
    None
}

/// Resolves the Steam Mini-Profile for `target_override` (or the user's configured / auto-detected
/// local `SteamID64`), caching for 10 minutes and degrading gracefully offline.
pub async fn resolve_steam_mini_profile(
    target_override: Option<String>,
    force_refresh: bool,
) -> Result<SteamMiniProfile, String> {
    let settings = crate::load_settings_standalone();
    let local_detected = detect_local_steam_user();
    let detected_id = local_detected.as_ref().map(|(id, _)| id.clone());
    let detected_persona = local_detected.and_then(|(_, p)| p);

    let explicit_override = target_override
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let settings_override = settings
        .steam_id_override
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);

    let chosen_raw = explicit_override
        .clone()
        .or_else(|| settings_override.clone())
        .or_else(|| detected_id.clone());

    let Some(raw_target) = chosen_raw else {
        return Ok(SteamMiniProfile {
            configured: false,
            persona_name: "Steam Profile".to_string(),
            online_state: "offline".to_string(),
            status_label: "No Steam Account Detected".to_string(),
            privacy_state: "public".to_string(),
            stranger_visibility_label:
                "Enter any SteamID64 or vanity URL to preview public profile".to_string(),
            profile_url: "https://steamcommunity.com".to_string(),
            offline_fallback: true,
            status_note: Some(
                "Sign into Steam locally or enter a SteamID64 / vanity URL to preview.".to_string(),
            ),
            fetched_at: now_unix_secs(),
            ..Default::default()
        });
    };

    let Some(target) = parse_steam_profile_target(&raw_target) else {
        return Err(format!(
            "Invalid SteamID64 or Steam Community URL: '{raw_target}'"
        ));
    };

    let is_override_preview = explicit_override.is_some()
        || (settings_override.is_some() && settings_override != detected_id);
    let cache_key = target.cache_key();

    if !force_refresh {
        if let Some(mut cached) = lookup_cached_profile(&cache_key, false) {
            cached.is_override_preview = is_override_preview;
            cached.detected_local_steam_id = detected_id;
            return Ok(cached);
        }
    }

    let xml_url = target.xml_url();
    match fetch_pooled_text(&xml_url, "application/xml, text/xml, */*").await {
        Ok(xml_body) => match parse_steam_community_xml(&xml_body, &target) {
            Ok(mut profile) => {
                profile.is_override_preview = is_override_preview;
                profile.detected_local_steam_id = detected_id.clone();
                if let Some(ref api_key) = settings.steam_api_key {
                    if !api_key.trim().is_empty() {
                        enrich_profile_with_web_api(&mut profile, api_key).await;
                    }
                }
                if !is_override_preview {
                    fill_local_library_showcase_fallback(&mut profile);
                }
                if let Ok(mut guard) = mem_cache().lock() {
                    guard.insert(cache_key.clone(), profile.clone());
                }
                save_disk_cache_entry(&cache_key, &profile);
                Ok(profile)
            }
            Err(parse_err) => {
                if let Some(mut stale) = lookup_cached_profile(&cache_key, true) {
                    stale.offline_fallback = true;
                    stale.is_override_preview = is_override_preview;
                    stale.detected_local_steam_id = detected_id;
                    stale.status_note = Some(format!("Showing cached profile ({parse_err})"));
                    return Ok(stale);
                }
                Ok(build_local_offline_fallback(
                    &target,
                    detected_id,
                    detected_persona,
                    is_override_preview,
                    &parse_err,
                ))
            }
        },
        Err(net_err) => {
            if let Some(mut stale) = lookup_cached_profile(&cache_key, true) {
                stale.offline_fallback = true;
                stale.is_override_preview = is_override_preview;
                stale.detected_local_steam_id = detected_id;
                stale.status_note = Some(format!("Offline — showing cached profile ({net_err})"));
                return Ok(stale);
            }
            Ok(build_local_offline_fallback(
                &target,
                detected_id,
                detected_persona,
                is_override_preview,
                &net_err,
            ))
        }
    }
}

fn build_local_offline_fallback(
    target: &SteamProfileTarget,
    detected_id: Option<String>,
    detected_persona: Option<String>,
    is_override_preview: bool,
    reason: &str,
) -> SteamMiniProfile {
    let sid = match target {
        SteamProfileTarget::SteamId64(id) => id.clone(),
        SteamProfileTarget::VanityId(_) => String::new(),
    };
    let persona = if !is_override_preview {
        detected_persona.unwrap_or_else(|| target.display_input())
    } else {
        target.display_input()
    };
    let mut fallback = SteamMiniProfile {
        configured: true,
        target_input: target.display_input(),
        steam_id64: sid,
        vanity_id: match target {
            SteamProfileTarget::VanityId(v) => Some(v.clone()),
            _ => None,
        },
        is_override_preview,
        detected_local_steam_id: detected_id,
        persona_name: persona,
        avatar_url: String::new(),
        online_state: "offline".to_string(),
        status_label: "Local Profile (Offline)".to_string(),
        summary_text: "Steam Community is currently unreachable; displaying local profile state."
            .to_string(),
        privacy_state: "public".to_string(),
        stranger_visibility_label: "Local Fallback — Connect to Preview Stranger Visibility"
            .to_string(),
        profile_url: target.web_profile_url(),
        showcase_games: Vec::new(),
        enriched_via_api_key: false,
        from_cache: false,
        offline_fallback: true,
        status_note: Some(format!("Offline fallback ({reason})")),
        fetched_at: now_unix_secs(),
        ..Default::default()
    };
    if !is_override_preview {
        fill_local_library_showcase_fallback(&mut fallback);
    }
    fallback
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_steam_profile_target_formats() {
        assert_eq!(
            parse_steam_profile_target("76561197960287930"),
            Some(SteamProfileTarget::SteamId64(
                "76561197960287930".to_string()
            ))
        );
        assert_eq!(
            parse_steam_profile_target("https://steamcommunity.com/profiles/76561197960287930/?xml=1"),
            Some(SteamProfileTarget::SteamId64(
                "76561197960287930".to_string()
            ))
        );
        assert_eq!(
            parse_steam_profile_target("https://steamcommunity.com/id/gabelogannewell/"),
            Some(SteamProfileTarget::VanityId("gabelogannewell".to_string()))
        );
        assert_eq!(
            parse_steam_profile_target("duke_player-99"),
            Some(SteamProfileTarget::VanityId("duke_player-99".to_string()))
        );
        assert_eq!(parse_steam_profile_target(""), None);
    }

    #[test]
    fn test_parse_steam_community_xml_public_and_showcase_games() {
        let sample_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<profile>
    <steamID64>76561197960287930</steamID64>
    <steamID><![CDATA[Duke Nukem]]></steamID>
    <onlineState>in-game</onlineState>
    <stateMessage><![CDATA[In-Game<br/>Elden Ring]]></stateMessage>
    <privacyState>public</privacyState>
    <visibilityState>3</visibilityState>
    <avatarFull><![CDATA[https://avatars.steamstatic.com/sample_full.jpg]]></avatarFull>
    <customURL><![CDATA[dukenukem]]></customURL>
    <memberSince>November 16, 2004</memberSince>
    <location><![CDATA[Austin, Texas, US]]></location>
    <realname><![CDATA[Duke]]></realname>
    <summary><![CDATA[Hail to the King!<br>Speedrunning &amp; co-op testing.]]></summary>
    <inGameInfo>
        <gameName><![CDATA[Elden Ring]]></gameName>
        <gameLink><![CDATA[https://steamcommunity.com/app/1245620]]></gameLink>
        <gameIcon><![CDATA[https://cdn.cloudflare.steamstatic.com/er_icon.jpg]]></gameIcon>
    </inGameInfo>
    <mostPlayedGames>
        <mostPlayedGame>
            <gameName><![CDATA[Elden Ring]]></gameName>
            <gameLink><![CDATA[https://steamcommunity.com/app/1245620]]></gameLink>
            <gameIcon><![CDATA[https://cdn.cloudflare.steamstatic.com/er_icon.jpg]]></gameIcon>
            <hoursPlayed>18.4</hoursPlayed>
            <hoursOnRecord>342</hoursOnRecord>
        </mostPlayedGame>
        <mostPlayedGame>
            <gameName><![CDATA[Baldur's Gate 3]]></gameName>
            <gameLink><![CDATA[https://steamcommunity.com/app/1086940]]></gameLink>
            <gameIcon><![CDATA[https://cdn.cloudflare.steamstatic.com/bg3_icon.jpg]]></gameIcon>
            <hoursPlayed>9.2</hoursPlayed>
            <hoursOnRecord>215</hoursOnRecord>
        </mostPlayedGame>
        <mostPlayedGame>
            <gameName><![CDATA[Hades II]]></gameName>
            <gameLink><![CDATA[https://steamcommunity.com/app/1145350]]></gameLink>
            <gameIcon><![CDATA[https://cdn.cloudflare.steamstatic.com/h2_icon.jpg]]></gameIcon>
            <hoursPlayed>4.0</hoursPlayed>
            <hoursOnRecord>88</hoursOnRecord>
        </mostPlayedGame>
        <mostPlayedGame>
            <gameName><![CDATA[Fourth Game Ignored]]></gameName>
            <gameLink><![CDATA[https://steamcommunity.com/app/400]]></gameLink>
            <hoursPlayed>1.0</hoursPlayed>
            <hoursOnRecord>10</hoursOnRecord>
        </mostPlayedGame>
    </mostPlayedGames>
</profile>"#;

        let target = SteamProfileTarget::SteamId64("76561197960287930".to_string());
        let profile = parse_steam_community_xml(sample_xml, &target).expect("valid XML");
        assert_eq!(profile.steam_id64, "76561197960287930");
        assert_eq!(profile.persona_name, "Duke Nukem");
        assert_eq!(profile.vanity_id.as_deref(), Some("dukenukem"));
        assert_eq!(profile.online_state, "in-game");
        assert_eq!(profile.status_label, "In-Game · Elden Ring");
        assert_eq!(profile.in_game_app_id, Some(1245620));
        assert_eq!(profile.privacy_state, "public");
        assert!(profile.stranger_visibility_label.contains("Public Profile"));
        assert_eq!(
            profile.summary_text,
            "Hail to the King!\nSpeedrunning & co-op testing."
        );
        assert_eq!(profile.member_since.as_deref(), Some("November 16, 2004"));
        assert_eq!(profile.showcase_games.len(), 3);
        assert_eq!(profile.showcase_games[0].app_id, Some(1245620));
        assert_eq!(profile.showcase_games[0].title, "Elden Ring");
        assert_eq!(profile.showcase_games[0].hours_two_weeks, "18.4");
        assert_eq!(profile.showcase_games[0].hours_total, "342");
        assert_eq!(profile.showcase_games[2].title, "Hades II");
    }

    #[test]
    fn test_parse_steam_community_xml_private_profile() {
        let private_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<profile>
    <steamID64>76561198000000001</steamID64>
    <steamID><![CDATA[StealthGamer]]></steamID>
    <privacyState>private</privacyState>
    <visibilityState>1</visibilityState>
    <avatarFull><![CDATA[https://avatars.steamstatic.com/priv.jpg]]></avatarFull>
</profile>"#;
        let target = SteamProfileTarget::SteamId64("76561198000000001".to_string());
        let profile = parse_steam_community_xml(private_xml, &target).expect("private XML");
        assert_eq!(profile.privacy_state, "private");
        assert!(profile
            .stranger_visibility_label
            .contains("Private Profile — Hidden From Strangers"));
        assert!(profile.showcase_games.is_empty());
    }

    #[test]
    fn test_parse_equipped_profile_items_json() {
        let payload = serde_json::json!({
            "response": {
                "animated_avatar": {
                    "item_title": "Neon Cyber Pulse",
                    "image_small": "items/1091500/anim_avatar.gif"
                },
                "avatar_frame": {
                    "item_title": "Golden Halo Frame",
                    "image_small": "items/1245620/frame_small.png"
                },
                "profile_background": {
                    "item_title": "Lands Between Vista",
                    "image_large": "items/1245620/bg_large.jpg"
                }
            }
        });
        let eq = parse_equipped_profile_items_json(&payload);
        assert_eq!(
            eq.animated_avatar_url.as_deref(),
            Some("https://cdn.cloudflare.steamstatic.com/steamcommunity/public/images/items/1091500/anim_avatar.gif")
        );
        assert_eq!(
            eq.avatar_frame_url.as_deref(),
            Some("https://cdn.cloudflare.steamstatic.com/steamcommunity/public/images/items/1245620/frame_small.png")
        );
        assert_eq!(eq.avatar_frame_title.as_deref(), Some("Golden Halo Frame"));
        assert_eq!(
            eq.profile_background_url.as_deref(),
            Some("https://cdn.cloudflare.steamstatic.com/steamcommunity/public/images/items/1245620/bg_large.jpg")
        );
        assert_eq!(
            eq.profile_background_title.as_deref(),
            Some("Lands Between Vista")
        );
    }
}
