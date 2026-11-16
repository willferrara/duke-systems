use crate::swarm::{self, SwarmAppAd};
use reqwest::header::ACCEPT;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::AppHandle;

const PSN_CLIENT_ID: &str = "09515159-7237-4370-9b40-3806e67c0891";
const PSN_REDIRECT_URI: &str = "com.scee.psxandroid.sceplink://redirect";
const PSN_BASIC_AUTH: &str =
    "Basic MDk1MTUxNTktNzIzNy00MzcwLTliNDAtMzgwNmU2N2MwODkxOnVjUGprYTV0bnRCMktxc1A=";

#[derive(Debug, Clone)]
struct PsnTokenCache {
    npsso_blake3: [u8; 32],
    access_token: String,
    expires_at_unix: u64,
}

static PSN_TOKEN_CACHE: OnceLock<Mutex<Option<PsnTokenCache>>> = OnceLock::new();
static FRIENDS_MEM_CACHE: OnceLock<Mutex<Option<(u64, UnifiedFriendsSnapshot)>>> = OnceLock::new();
const FRIENDS_CACHE_TTL_SECS: u64 = 12;

fn friends_mem_cache() -> &'static Mutex<Option<(u64, UnifiedFriendsSnapshot)>> {
    FRIENDS_MEM_CACHE.get_or_init(|| Mutex::new(None))
}

pub fn clear_friends_cache() {
    *friends_mem_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
    let slot = PSN_TOKEN_CACHE.get_or_init(|| Mutex::new(None));
    *slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct UnifiedFriend {
    pub id: String,
    pub platform: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub hardware_badge: Option<String>,
    pub status: String,
    pub status_label: String,
    pub presence_text: String,
    pub game_title: Option<String>,
    pub game_id: Option<String>,
    pub lobby_steam_id: Option<String>,
    pub game_server_ip: Option<String>,
    pub can_join_lobby: bool,
    pub matched_library_game_id: Option<String>,
    pub swarm_peer_addr: Option<String>,
    pub swarm_apps: Vec<SwarmAppAd>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct UnifiedFriendsCounts {
    pub in_game: usize,
    pub online: usize,
    pub offline: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PlatformFriendState {
    pub steam: String,
    pub xbox: String,
    pub psn: String,
    pub swarm: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct UnifiedFriendsSnapshot {
    pub in_game: Vec<UnifiedFriend>,
    pub online: Vec<UnifiedFriend>,
    pub offline: Vec<UnifiedFriend>,
    pub counts: UnifiedFriendsCounts,
    pub platform_status: PlatformFriendState,
    pub detected_steam_id: Option<String>,
    pub fetched_at: u64,
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn is_valid_steam_id64(raw: &str) -> bool {
    let trimmed = raw.trim();
    trimmed.len() == 17
        && trimmed.bytes().all(|b| b.is_ascii_digit())
        && trimmed.starts_with("7656119")
}

/// Extracts the most recent 64-bit SteamID from Valve's `Steam/config/loginusers.vdf`.
pub fn parse_steam_id_from_loginusers_vdf(vdf_text: &str) -> Option<String> {
    let tree = crate::importers::parse_vdf_kv_tree(vdf_text);
    let users = tree.get_node("users").or(Some(&tree)).and_then(|n| n.as_map())?;

    let mut best: Option<(bool, u64, String)> = None;
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

        let replace = match &best {
            None => true,
            Some((prev_recent, prev_ts, _)) => {
                (most_recent, timestamp) > (*prev_recent, *prev_ts)
            }
        };
        if replace {
            best = Some((most_recent, timestamp, candidate_id.to_string()));
        }
    }

    best.map(|(_, _, id)| id)
}

fn find_steam_install_roots() -> Vec<PathBuf> {
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
                duke_core::win32_ffi::reg_read_string(true, "SOFTWARE\\Valve\\Steam", "InstallPath")
            })
    {
        let p = PathBuf::from(reg_path.replace('/', "\\"));
        if p.is_dir() {
            roots.push(p);
        }
    }

    for env_key in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
        if let Ok(base) = std::env::var(env_key) {
            let cand = PathBuf::from(base).join("Steam");
            if cand.is_dir()
                && !roots
                    .iter()
                    .any(|x| x.to_string_lossy().eq_ignore_ascii_case(&cand.to_string_lossy()))
            {
                roots.push(cand);
            }
        }
    }
    roots
}

pub fn detect_local_steam_id64() -> Option<String> {
    for root in find_steam_install_roots() {
        let vdf_path = root.join("config").join("loginusers.vdf");
        if let Ok(raw) = fs::read_to_string(&vdf_path) {
            if let Some(id) = parse_steam_id_from_loginusers_vdf(&raw) {
                return Some(id);
            }
        }
    }
    None
}

pub(crate) fn resolve_effective_steam_id64(settings: &crate::LauncherSettings) -> Option<String> {
    settings
        .steam_id_override
        .as_deref()
        .map(str::trim)
        .and_then(|s| match crate::steam_profile::parse_steam_profile_target(s) {
            Some(crate::steam_profile::SteamProfileTarget::SteamId64(id)) => Some(id),
            _ if is_valid_steam_id64(s) => Some(s.to_string()),
            _ => None,
        })
        .or_else(detect_local_steam_id64)
}

pub(crate) async fn fetch_doh_json_redacted(
    url: &str,
    headers: &[(&str, &str)],
    label: &str,
) -> Result<Value, String> {
    let client = duke_core::net_doh::build_doh_client_with_timeouts(
        url,
        Duration::from_secs(7),
        Duration::from_secs(9),
        true,
    )
    .await;
    let mut req = client.get(url).header(ACCEPT, "application/json");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = req
        .send()
        .await
        .map_err(|_| format!("{label}: network request failed"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("{label}: HTTP {}", status.as_u16()));
    }
    resp.json::<Value>()
        .await
        .map_err(|_| format!("{label}: invalid JSON response"))
}

fn match_library_game(
    library: &[crate::GameEntry],
    steam_app_id: Option<&str>,
    title: Option<&str>,
) -> Option<String> {
    if let Some(raw_aid) = steam_app_id.map(str::trim).filter(|s| !s.is_empty()) {
        if let Ok(aid_u32) = raw_aid.parse::<u32>() {
            if let Some(found) = library
                .iter()
                .find(|g| g.app_id == Some(aid_u32) || g.steam_app_id == Some(aid_u32 as u64))
            {
                return Some(found.id.clone());
            }
        }
    }

    if let Some(raw_title) = title.map(str::trim).filter(|s| !s.is_empty()) {
        let target = crate::clean_game_title(raw_title).to_ascii_lowercase();
        if !target.is_empty() {
            if let Some(found) = library.iter().find(|g| {
                let cleaned = crate::clean_game_title(&g.title).to_ascii_lowercase();
                cleaned == target || g.title.trim().eq_ignore_ascii_case(raw_title)
            }) {
                return Some(found.id.clone());
            }
        }
    }

    None
}

fn steam_personastate_label(state: u64, has_game: bool) -> (&'static str, &'static str) {
    if state == 0 {
        ("offline", "Offline")
    } else if has_game {
        ("in_game", "In-Game")
    } else {
        match state {
            2 => ("online", "Busy"),
            3 => ("online", "Away"),
            4 => ("online", "Snooze"),
            5 => ("online", "Looking to Trade"),
            6 => ("online", "Looking to Play"),
            _ => ("online", "Online"),
        }
    }
}

pub fn classify_steam_friends_error(err: &str, api_key_verified_valid: bool) -> &'static str {
    let lower = err.to_ascii_lowercase();
    if lower.contains("no local steamid64") {
        "No Local SteamID64"
    } else if lower.contains("invalid api key") || lower.contains("http 403") {
        "Invalid API Key"
    } else if lower.contains("http 401") {
        if api_key_verified_valid {
            "Steam Profile Private"
        } else {
            "Invalid API Key"
        }
    } else if lower.contains("private") || lower.contains("unavailable") {
        "Steam Profile Private"
    } else {
        "Network Unreachable"
    }
}

pub fn parse_friend_list_v1_json(list_json: &Value) -> Result<Vec<String>, String> {
    let friends_arr = list_json
        .get("friendslist")
        .and_then(|v| v.get("friends"))
        .or_else(|| list_json.get("response").and_then(|v| v.get("friends")))
        .and_then(|v| v.as_array())
        .ok_or_else(|| "Steam Profile Private".to_string())?;

    let mut steam_ids: Vec<String> = friends_arr
        .iter()
        .filter_map(|item| item.get("steamid").and_then(|s| s.as_str()))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    steam_ids.truncate(300);
    Ok(steam_ids)
}

pub fn parse_player_summaries_friends_v2_json(
    sum_json: &Value,
    library: &[crate::GameEntry],
) -> Vec<UnifiedFriend> {
    let Some(players) = sum_json
        .get("response")
        .and_then(|r| r.get("players"))
        .or_else(|| sum_json.get("players").and_then(|p| p.get("player")))
        .and_then(|p| p.as_array())
    else {
        return Vec::new();
    };

    let mut out = Vec::with_capacity(players.len());
    for player in players {
        let friend_sid = player
            .get("steamid")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if friend_sid.is_empty() {
            continue;
        }
        let name = player
            .get("personaname")
            .and_then(|v| v.as_str())
            .unwrap_or("Steam User")
            .trim()
            .to_string();
        let avatar = player
            .get("avatarfull")
            .or_else(|| player.get("avatarmedium"))
            .or_else(|| player.get("avatar"))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let personastate = player
            .get("personastate")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let game_title = player
            .get("gameextrainfo")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let game_id = player
            .get("gameid")
            .and_then(|v| {
                v.as_str()
                    .map(str::to_string)
                    .or_else(|| v.as_u64().map(|n| n.to_string()))
            })
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s != "0");
        let lobby_steam_id = player
            .get("lobbysteamid")
            .and_then(|v| {
                v.as_str()
                    .map(str::to_string)
                    .or_else(|| v.as_u64().map(|n| n.to_string()))
            })
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s != "0");
        let game_server_ip = player
            .get("gameserverip")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s != "0.0.0.0:0");

        let has_game = personastate != 0 && (game_title.is_some() || game_id.is_some());
        let (status, status_label) = steam_personastate_label(personastate, has_game);
        let presence_text = match (&game_title, status) {
            (Some(gt), "in_game") => format!("Playing {gt}"),
            (_, "offline") => "Offline".to_string(),
            _ => status_label.to_string(),
        };
        let can_join_lobby =
            (lobby_steam_id.is_some() && game_id.is_some()) || game_server_ip.is_some();
        let matched_library_game_id =
            match_library_game(library, game_id.as_deref(), game_title.as_deref());

        out.push(UnifiedFriend {
            id: format!("steam:{friend_sid}"),
            platform: "Steam".to_string(),
            display_name: name,
            avatar_url: avatar,
            hardware_badge: None,
            status: status.to_string(),
            status_label: status_label.to_string(),
            presence_text,
            game_title,
            game_id,
            lobby_steam_id,
            game_server_ip,
            can_join_lobby,
            matched_library_game_id,
            swarm_peer_addr: None,
            swarm_apps: Vec::new(),
        });
    }
    out
}

async fn fetch_steam_friends(
    api_key: &str,
    steam_id64: &str,
    library: &[crate::GameEntry],
) -> Result<Vec<UnifiedFriend>, String> {
    let key_enc = urlencoding::encode(api_key.trim());
    let sid_enc = urlencoding::encode(steam_id64.trim());
    let list_url = format!(
        "https://api.steampowered.com/ISteamUser/GetFriendList/v1/?key={key_enc}&steamid={sid_enc}&relationship=friend"
    );
    let self_sum_url = format!(
        "https://api.steampowered.com/ISteamUser/GetPlayerSummaries/v2/?key={key_enc}&steamids={sid_enc}"
    );

    let list_json = match fetch_doh_json_redacted(&list_url, &[], "Steam Friends List").await {
        Ok(json) => json,
        Err(err) => {
            if err.contains("HTTP 401") || err.contains("HTTP 403") {
                let key_ok = fetch_doh_json_redacted(&self_sum_url, &[], "Steam Key Probe")
                    .await
                    .is_ok();
                return Err(classify_steam_friends_error(&err, key_ok).to_string());
            }
            return Err(classify_steam_friends_error(&err, false).to_string());
        }
    };

    let steam_ids = match parse_friend_list_v1_json(&list_json) {
        Ok(ids) => ids,
        Err(parse_err) => {
            let key_ok = fetch_doh_json_redacted(&self_sum_url, &[], "Steam Key Probe")
                .await
                .is_ok();
            return Err(classify_steam_friends_error(&parse_err, key_ok).to_string());
        }
    };

    if steam_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut out = Vec::with_capacity(steam_ids.len());
    for chunk in steam_ids.chunks(100) {
        let joined = chunk.join(",");
        let sum_url = format!(
            "https://api.steampowered.com/ISteamUser/GetPlayerSummaries/v2/?key={key_enc}&steamids={}",
            urlencoding::encode(&joined)
        );
        let sum_json = fetch_doh_json_redacted(&sum_url, &[], "Steam Player Summaries")
            .await
            .map_err(|e| classify_steam_friends_error(&e, false).to_string())?;
        out.extend(parse_player_summaries_friends_v2_json(&sum_json, library));
    }

    Ok(out)
}

async fn fetch_xbox_friends(
    api_key: &str,
    library: &[crate::GameEntry],
) -> Result<Vec<UnifiedFriend>, String> {
    let url = "https://xbl.io/api/v2/friends";
    let payload = fetch_doh_json_redacted(url, &[("X-Authorization", api_key.trim())], "Xbox Live API").await?;
    let people = payload
        .get("people")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "Xbox Live API: missing people list".to_string())?;

    let mut out = Vec::with_capacity(people.len());
    for person in people {
        let xuid = person
            .get("xuid")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if xuid.is_empty() {
            continue;
        }
        let gamertag = person
            .get("gamertag")
            .or_else(|| person.get("displayName"))
            .and_then(|v| v.as_str())
            .unwrap_or("Xbox Player")
            .trim()
            .to_string();
        let avatar_url = person
            .get("displayPicRaw")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let presence_state = person
            .get("presenceState")
            .and_then(|v| v.as_str())
            .unwrap_or("Offline")
            .trim();
        let presence_text_raw = person
            .get("presenceText")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        let mut active_title: Option<String> = None;
        let mut rich_detail: Option<String> = None;
        if let Some(details) = person.get("presenceDetails").and_then(|v| v.as_array()) {
            for item in details {
                let state = item.get("State").and_then(|v| v.as_str()).unwrap_or("");
                let title_name = item
                    .get("TitleName")
                    .or_else(|| item.get("Title"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| {
                        !s.is_empty()
                            && !s.eq_ignore_ascii_case("Online")
                            && !s.eq_ignore_ascii_case("Home")
                    });
                let rich = item
                    .get("RichPresenceText")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                if state.eq_ignore_ascii_case("Active") || active_title.is_none() {
                    if let Some(tn) = title_name {
                        active_title = Some(tn.to_string());
                    }
                    if let Some(r) = rich {
                        rich_detail = Some(r.to_string());
                    }
                }
            }
        }

        if active_title.is_none()
            && !presence_text_raw.is_empty()
            && !presence_text_raw.eq_ignore_ascii_case("Online")
            && !presence_text_raw.eq_ignore_ascii_case("Offline")
            && !presence_text_raw.eq_ignore_ascii_case("Home")
            && !presence_text_raw.to_ascii_lowercase().starts_with("last seen")
        {
            active_title = Some(presence_text_raw.clone());
        }

        let lower_state = presence_state.to_ascii_lowercase();
        let (status, status_label) = if lower_state == "online" && active_title.is_some() {
            ("in_game", "In-Game")
        } else if lower_state == "online" || lower_state == "away" {
            ("online", if lower_state == "away" { "Away" } else { "Online" })
        } else {
            ("offline", "Offline")
        };

        let presence_text = match (&active_title, &rich_detail) {
            (Some(t), Some(r)) if r != t => format!("{t} — {r}"),
            (Some(t), _) => format!("Playing {t}"),
            _ if !presence_text_raw.is_empty() => presence_text_raw,
            _ => status_label.to_string(),
        };

        let matched_library_game_id = match_library_game(library, None, active_title.as_deref());

        out.push(UnifiedFriend {
            id: format!("xbox:{xuid}"),
            platform: "Xbox".to_string(),
            display_name: gamertag,
            avatar_url,
            hardware_badge: Some("Xbox".to_string()),
            status: status.to_string(),
            status_label: status_label.to_string(),
            presence_text,
            game_title: active_title,
            game_id: None,
            lobby_steam_id: None,
            game_server_ip: None,
            can_join_lobby: false,
            matched_library_game_id,
            swarm_peer_addr: None,
            swarm_apps: Vec::new(),
        });
    }

    Ok(out)
}

pub(crate) async fn fetch_psn_access_token(npsso: &str) -> Result<String, String> {
    let trimmed = npsso.trim();
    if trimmed.is_empty() {
        return Err("PSN npsso token not configured".to_string());
    }
    let npsso_hash = *blake3::hash(trimmed.as_bytes()).as_bytes();
    let now = now_unix_secs();

    {
        let slot = PSN_TOKEN_CACHE.get_or_init(|| Mutex::new(None));
        let guard = slot.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref cached) = *guard {
            if cached.npsso_blake3 == npsso_hash && cached.expires_at_unix > now + 60 {
                return Ok(cached.access_token.clone());
            }
        }
    }

    let auth_url = format!(
        "https://ca.account.sony.com/api/authz/v3/oauth/authorize?access_type=offline&client_id={PSN_CLIENT_ID}&redirect_uri={}&response_type=code&scope={}",
        urlencoding::encode(PSN_REDIRECT_URI),
        urlencoding::encode("psn:mobile.v2.core psn:clientapp")
    );
    let auth_client = duke_core::net_doh::build_doh_client_with_timeouts(
        &auth_url,
        Duration::from_secs(7),
        Duration::from_secs(9),
        false,
    )
    .await;
    let auth_resp = auth_client
        .get(&auth_url)
        .header("Cookie", format!("npsso={trimmed}"))
        .send()
        .await
        .map_err(|_| "PSN OAuth authorize request failed".to_string())?;

    let location = auth_resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| "PSN npsso token expired or invalid (missing OAuth redirect)".to_string())?;

    let code = reqwest::Url::parse(location)
        .ok()
        .and_then(|u| {
            u.query_pairs()
                .find(|(k, _)| k == "code")
                .map(|(_, v)| v.into_owned())
        })
        .filter(|c| !c.is_empty())
        .ok_or_else(|| "PSN OAuth redirect did not yield an authorization code".to_string())?;

    let token_url = "https://ca.account.sony.com/api/authz/v3/oauth/token";
    let token_client = duke_core::net_doh::build_doh_client_with_timeouts(
        token_url,
        Duration::from_secs(7),
        Duration::from_secs(9),
        true,
    )
    .await;
    let token_resp = token_client
        .post(token_url)
        .header("Authorization", PSN_BASIC_AUTH)
        .form(&[
            ("code", code.as_str()),
            ("redirect_uri", PSN_REDIRECT_URI),
            ("grant_type", "authorization_code"),
            ("token_format", "jwt"),
        ])
        .send()
        .await
        .map_err(|_| "PSN OAuth token exchange failed".to_string())?;

    if !token_resp.status().is_success() {
        return Err(format!(
            "PSN OAuth token exchange HTTP {}",
            token_resp.status().as_u16()
        ));
    }
    let token_json: Value = token_resp
        .json()
        .await
        .map_err(|_| "PSN OAuth token response was not valid JSON".to_string())?;
    let access_token = token_json
        .get("access_token")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "PSN OAuth response missing access_token".to_string())?
        .to_string();
    let expires_in = token_json
        .get("expires_in")
        .and_then(|v| v.as_u64())
        .unwrap_or(3600);

    let slot = PSN_TOKEN_CACHE.get_or_init(|| Mutex::new(None));
    {
        let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(PsnTokenCache {
            npsso_blake3: npsso_hash,
            access_token: access_token.clone(),
            expires_at_unix: now_unix_secs().saturating_add(expires_in),
        });
    }

    Ok(access_token)
}

async fn fetch_psn_friends(
    npsso: &str,
    library: &[crate::GameEntry],
) -> Result<Vec<UnifiedFriend>, String> {
    let bearer_token = fetch_psn_access_token(npsso).await?;
    let auth_header = format!("Bearer {bearer_token}");
    let headers = [("Authorization", auth_header.as_str())];

    let friends_url = "https://m.np.playstation.com/api/userProfile/v1/internal/users/me/friends?limit=100";
    let friends_json = fetch_doh_json_redacted(friends_url, &headers, "PSN Friends").await?;
    let account_ids: Vec<String> = friends_json
        .get("friends")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .or_else(|| v.as_u64().map(|n| n.to_string()))
                })
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    if account_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut profiles_by_id: HashMap<String, (String, Option<String>)> = HashMap::new();
    let mut presences_by_id: HashMap<String, (String, Option<String>, Option<String>)> =
        HashMap::new();

    for chunk in account_ids.chunks(50) {
        let joined = chunk.join(",");
        let enc = urlencoding::encode(&joined);

        let prof_url = format!(
            "https://m.np.playstation.com/api/userProfile/v1/internal/users/profiles?accountIds={enc}"
        );
        if let Ok(prof_json) = fetch_doh_json_redacted(&prof_url, &headers, "PSN Profiles").await {
            if let Some(arr) = prof_json.get("profiles").and_then(|v| v.as_array()) {
                for (idx, item) in arr.iter().enumerate() {
                    let aid = item
                        .get("accountId")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                        .or_else(|| chunk.get(idx).cloned())
                        .unwrap_or_default();
                    let online_id = item
                        .get("onlineId")
                        .and_then(|v| v.as_str())
                        .unwrap_or("PSN Player")
                        .trim()
                        .to_string();
                    let avatar = item
                        .get("avatars")
                        .or_else(|| item.get("avatarUrls"))
                        .and_then(|v| v.as_array())
                        .and_then(|list| {
                            list.iter()
                                .find(|a| a.get("size").and_then(|s| s.as_str()) == Some("xl"))
                                .or_else(|| {
                                    list.iter().find(|a| {
                                        a.get("size").and_then(|s| s.as_str()) == Some("l")
                                    })
                                })
                                .or_else(|| list.first())
                                .and_then(|a| {
                                    a.get("url")
                                        .or_else(|| a.get("avatarUrl"))
                                        .and_then(|u| u.as_str())
                                })
                        })
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty());
                    if !aid.is_empty() {
                        profiles_by_id.insert(aid, (online_id, avatar));
                    }
                }
            }
        }

        let pres_url = format!(
            "https://m.np.playstation.com/api/userProfile/v1/internal/users/basicPresences?type=primary&accountIds={enc}"
        );
        if let Ok(pres_json) = fetch_doh_json_redacted(&pres_url, &headers, "PSN Presence").await {
            if let Some(arr) = pres_json
                .get("basicPresences")
                .or_else(|| pres_json.get("presences"))
                .and_then(|v| v.as_array())
            {
                for item in arr {
                    let aid = item
                        .get("accountId")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    if aid.is_empty() {
                        continue;
                    }
                    let primary = item.get("primaryPlatformInfo");
                    let online_status = primary
                        .and_then(|p| p.get("onlineStatus"))
                        .or_else(|| item.get("availability"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("offline")
                        .trim()
                        .to_string();
                    let platform_hw = primary
                        .and_then(|p| p.get("platform"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_uppercase())
                        .filter(|s| !s.is_empty());
                    let title_name = item
                        .get("gameTitleInfoList")
                        .and_then(|v| v.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|t| t.get("titleName"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty());

                    presences_by_id.insert(aid, (online_status, platform_hw, title_name));
                }
            }
        }
    }

    let mut out = Vec::with_capacity(account_ids.len());
    for aid in account_ids {
        let (display_name, avatar_url) = profiles_by_id
            .get(&aid)
            .cloned()
            .unwrap_or_else(|| (format!("PSN #{aid}"), None));
        let (online_status, hw_badge, game_title) = presences_by_id
            .get(&aid)
            .cloned()
            .unwrap_or_else(|| ("offline".to_string(), None, None));

        let is_online = online_status.eq_ignore_ascii_case("online")
            || online_status.eq_ignore_ascii_case("availableToPlay");
        let (status, status_label) = if is_online && game_title.is_some() {
            ("in_game", "In-Game")
        } else if is_online {
            ("online", "Online")
        } else {
            ("offline", "Offline")
        };

        let presence_text = match (&game_title, status) {
            (Some(gt), "in_game") => format!("Playing {gt}"),
            (_, "online") => hw_badge
                .as_deref()
                .map(|hw| format!("Online on {hw}"))
                .unwrap_or_else(|| "Online".to_string()),
            _ => "Offline".to_string(),
        };

        let matched_library_game_id = match_library_game(library, None, game_title.as_deref());

        out.push(UnifiedFriend {
            id: format!("psn:{aid}"),
            platform: "PSN".to_string(),
            display_name,
            avatar_url,
            hardware_badge: hw_badge,
            status: status.to_string(),
            status_label: status_label.to_string(),
            presence_text,
            game_title,
            game_id: None,
            lobby_steam_id: None,
            game_server_ip: None,
            can_join_lobby: false,
            matched_library_game_id,
            swarm_peer_addr: None,
            swarm_apps: Vec::new(),
        });
    }

    Ok(out)
}

fn collect_swarm_friends(library: &[crate::GameEntry]) -> (Vec<UnifiedFriend>, String) {
    let snap = swarm::snapshot_swarm_status_public();
    if !snap.enabled && snap.peers.is_empty() {
        return (Vec::new(), "Disabled".to_string());
    }

    let mut out = Vec::with_capacity(snap.peers.len());
    for peer in &snap.peers {
        let peer_addr = format!("{}:{}", peer.addr, peer.tcp_port);
        let badge = if peer.transport.to_ascii_lowercase().contains("tailscale") {
            "Tailscale"
        } else {
            "LAN"
        };

        let matched_lib_id = peer
            .apps
            .iter()
            .find_map(|a| {
                let appid_str = a.app_id.map(|id| id.to_string());
                match_library_game(library, appid_str.as_deref(), Some(&a.title))
            })
            .or_else(|| {
                library
                    .iter()
                    .find(|g| peer.save_clocks.contains_key(&g.id))
                    .map(|g| g.id.clone())
            });

        let app_summary = if peer.apps.is_empty() {
            format!("Duke Swarm {badge} peer ({peer_addr})")
        } else {
            format!(
                "{} shared game{} on {badge}",
                peer.apps.len(),
                if peer.apps.len() == 1 { "" } else { "s" }
            )
        };

        out.push(UnifiedFriend {
            id: format!("swarm:{}", peer.node_id),
            platform: "Swarm".to_string(),
            display_name: peer.node_name.clone(),
            avatar_url: None,
            hardware_badge: Some(badge.to_string()),
            status: "online".to_string(),
            status_label: "Swarm Online".to_string(),
            presence_text: app_summary,
            game_title: peer.apps.first().map(|a| a.title.clone()),
            game_id: peer.apps.first().and_then(|a| a.app_id.map(|id| id.to_string())),
            lobby_steam_id: None,
            game_server_ip: None,
            can_join_lobby: false,
            matched_library_game_id: matched_lib_id,
            swarm_peer_addr: Some(peer_addr),
            swarm_apps: peer.apps.clone(),
        });
    }

    let status = if snap.peers.is_empty() {
        "Listening (0 peers)".to_string()
    } else {
        format!("{} online", snap.peers.len())
    };
    (out, status)
}

pub fn build_steam_join_uri(
    steam_id: Option<&str>,
    app_id: Option<&str>,
    lobby_steam_id: Option<&str>,
    game_server_ip: Option<&str>,
) -> Result<String, String> {
    let lobby = lobby_steam_id
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "0" && s.bytes().all(|b| b.is_ascii_digit()));
    let aid = app_id
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "0" && s.bytes().all(|b| b.is_ascii_digit()));
    let sid = steam_id
        .map(str::trim)
        .map(|s| s.strip_prefix("steam:").unwrap_or(s))
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));

    if let (Some(app), Some(lob), Some(friend)) = (aid, lobby, sid) {
        return Ok(format!("steam://joinlobby/{app}/{lob}/{friend}"));
    }

    if let Some(ip_raw) = game_server_ip.map(str::trim).filter(|s| !s.is_empty()) {
        let valid_ip = ip_raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b':' | b'-' | b'[' | b']'));
        if !valid_ip || ip_raw == "0.0.0.0:0" {
            return Err("Invalid game server IP address.".to_string());
        }
        if let Some(app) = aid {
            return Ok(format!(
                "steam://rungameid/{app}//+connect%20{}",
                urlencoding::encode(ip_raw)
            ));
        }
        return Ok(format!("steam://connect/{ip_raw}"));
    }

    Err("Friend has no active joinable Steam lobby or game server IP.".to_string())
}

#[tauri::command]
pub async fn get_unified_friends(app: AppHandle) -> Result<UnifiedFriendsSnapshot, String> {
    let now = now_unix_secs();
    {
        let guard = friends_mem_cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some((cached_at, ref snap)) = *guard {
            if now.saturating_sub(cached_at) < FRIENDS_CACHE_TTL_SECS {
                return Ok(snap.clone());
            }
        }
    }

    let settings = crate::load_settings_standalone();
    let library = crate::load_library_from_disk(&app).unwrap_or_default();

    let detected_steam_id = detect_local_steam_id64();
    let resolved_steam_id = resolve_effective_steam_id64(&settings);

    let steam_key = settings
        .steam_api_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let xbox_key = settings
        .xbox_api_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let psn_npsso = settings
        .psn_npsso
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);

    let lib_steam = library.clone();
    let lib_xbox = library.clone();
    let lib_psn = library.clone();

    let steam_fut = async move {
        match (steam_key, resolved_steam_id) {
            (Some(k), Some(sid)) => match fetch_steam_friends(&k, &sid, &lib_steam).await {
                Ok(list) => (list, "Connected".to_string()),
                Err(e) => (Vec::new(), e),
            },
            (_, None) => (Vec::new(), "No Local SteamID64".to_string()),
            (None, Some(_)) => (Vec::new(), "No Steam Web API key".to_string()),
        }
    };

    let xbox_fut = async move {
        match xbox_key {
            Some(k) => match fetch_xbox_friends(&k, &lib_xbox).await {
                Ok(list) => (list, "Connected".to_string()),
                Err(e) => (Vec::new(), e),
            },
            None => (Vec::new(), "No Xbox API key".to_string()),
        }
    };

    let psn_fut = async move {
        match psn_npsso {
            Some(token) => match fetch_psn_friends(&token, &lib_psn).await {
                Ok(list) => (list, "Connected".to_string()),
                Err(e) => (Vec::new(), e),
            },
            None => (Vec::new(), "No PSN npsso token".to_string()),
        }
    };

    let ((steam_list, steam_state), (xbox_list, xbox_state), (psn_list, psn_state)) =
        tokio::join!(steam_fut, xbox_fut, psn_fut);
    let (swarm_list, swarm_state) = collect_swarm_friends(&library);

    let mut all = Vec::with_capacity(
        steam_list.len() + xbox_list.len() + psn_list.len() + swarm_list.len(),
    );
    all.extend(steam_list);
    all.extend(xbox_list);
    all.extend(psn_list);
    all.extend(swarm_list);

    let mut in_game = Vec::new();
    let mut online = Vec::new();
    let mut offline = Vec::new();

    for friend in all {
        match friend.status.as_str() {
            "in_game" => in_game.push(friend),
            "online" => online.push(friend),
            _ => offline.push(friend),
        }
    }

    in_game.sort_by_key(|a| a.display_name.to_lowercase());
    online.sort_by_key(|a| a.display_name.to_lowercase());
    offline.sort_by_key(|a| a.display_name.to_lowercase());

    let counts = UnifiedFriendsCounts {
        in_game: in_game.len(),
        online: online.len(),
        offline: offline.len(),
        total: in_game.len() + online.len() + offline.len(),
    };

    let snapshot = UnifiedFriendsSnapshot {
        in_game,
        online,
        offline,
        counts,
        platform_status: PlatformFriendState {
            steam: steam_state,
            xbox: xbox_state,
            psn: psn_state,
            swarm: swarm_state,
        },
        detected_steam_id,
        fetched_at: now_unix_secs(),
    };

    {
        let mut guard = friends_mem_cache().lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some((snapshot.fetched_at, snapshot.clone()));
    }

    Ok(snapshot)
}

#[tauri::command]
pub async fn join_friend_session(
    steam_id: Option<String>,
    app_id: Option<String>,
    lobby_steam_id: Option<String>,
    game_server_ip: Option<String>,
) -> Result<String, String> {
    let uri = build_steam_join_uri(
        steam_id.as_deref(),
        app_id.as_deref(),
        lobby_steam_id.as_deref(),
        game_server_ip.as_deref(),
    )?;
    open::that(&uri).map_err(|e| format!("Failed launching Steam join URI: {e}"))?;
    Ok(uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_steam_id64_from_loginusers_vdf_by_most_recent_and_timestamp() {
        let vdf = r#"
"users"
{
    "76561198000000001"
    {
        "AccountName"       "older_user"
        "PersonaName"       "Older"
        "RememberPassword"  "1"
        "MostRecent"        "0"
        "Timestamp"         "1700000100"
    }
    "76561198012345678"
    {
        "AccountName"       "active_user"
        "PersonaName"       "ActivePlayer"
        "RememberPassword"  "1"
        "MostRecent"        "1"
        "Timestamp"         "1710000500"
    }
}
"#;
        assert_eq!(
            parse_steam_id_from_loginusers_vdf(vdf),
            Some("76561198012345678".to_string())
        );

        let fallback_vdf = "\u{feff}\"users\"\n{\n  \"76561198000000002\" { \"Timestamp\" \"1700000900\" }\n  \"76561198000000001\" { \"Timestamp\" \"1700000100\" }\n  \"invalid_id\" { \"MostRecent\" \"1\" }\n}\n";
        assert_eq!(
            parse_steam_id_from_loginusers_vdf(fallback_vdf),
            Some("76561198000000002".to_string())
        );
    }

    #[test]
    fn builds_steam_join_uri_for_lobby_and_server_ip() {
        let lobby = build_steam_join_uri(
            Some("steam:76561198012345678"),
            Some("730"),
            Some("109775241234567890"),
            None,
        )
        .unwrap();
        assert_eq!(
            lobby,
            "steam://joinlobby/730/109775241234567890/76561198012345678"
        );

        let server = build_steam_join_uri(
            Some("76561198012345678"),
            Some("4000"),
            None,
            Some("198.51.100.42:27015"),
        )
        .unwrap();
        assert_eq!(
            server,
            "steam://rungameid/4000//+connect%20198.51.100.42%3A27015"
        );

        assert!(build_steam_join_uri(None, None, None, Some("bad host;calc")).is_err());
    }

    #[test]
    fn parses_friend_list_v1_and_player_summaries_v2_and_classifies_diagnostics() {
        let list_json = serde_json::json!({
            "friendslist": {
                "friends": [
                    { "steamid": "76561198000000011", "relationship": "friend" },
                    { "steamid": "76561198000000022", "relationship": "friend" }
                ]
            }
        });
        let ids = parse_friend_list_v1_json(&list_json).expect("valid friendlist");
        assert_eq!(ids, vec!["76561198000000011", "76561198000000022"]);

        let sum_json = serde_json::json!({
            "response": {
                "players": [{
                    "steamid": "76561198000000011",
                    "personaname": "CoopBuddy",
                    "avatarfull": "https://avatars.steamstatic.com/cb.jpg",
                    "personastate": 1,
                    "gameextrainfo": "Helldivers 2",
                    "gameid": "553850"
                }]
            }
        });
        let friends = parse_player_summaries_friends_v2_json(&sum_json, &[]);
        assert_eq!(friends.len(), 1);
        assert_eq!(friends[0].display_name, "CoopBuddy");
        assert_eq!(friends[0].status, "in_game");
        assert_eq!(friends[0].presence_text, "Playing Helldivers 2");

        assert_eq!(classify_steam_friends_error("Steam Friends List: HTTP 401", true), "Steam Profile Private");
        assert_eq!(classify_steam_friends_error("Steam Friends List: HTTP 401", false), "Invalid API Key");
        assert_eq!(classify_steam_friends_error("Steam Friends List: HTTP 403", true), "Invalid API Key");
        assert_eq!(classify_steam_friends_error("No Local SteamID64", false), "No Local SteamID64");
        assert_eq!(classify_steam_friends_error("network request failed", false), "Network Unreachable");

        clear_friends_cache();
        assert!(friends_mem_cache().lock().unwrap_or_else(|e| e.into_inner()).is_none());
    }
}
