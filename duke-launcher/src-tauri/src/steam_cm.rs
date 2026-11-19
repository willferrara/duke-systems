use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::AppHandle;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

const STEAM_DIR_CM_URL: &str =
    "https://api.steampowered.com/ISteamDirectory/GetCMListForConnect/v1/?cellid=0&cmtype=websockets";
const STEAM_GEN_TOKEN_URL: &str =
    "https://api.steampowered.com/IAuthenticationService/GenerateAccessTokenForApp/v1/";
const STEAM_SEND_CHAT_URL: &str =
    "https://api.steampowered.com/IFriendMessagesService/SendMessage/v1/";
const STEAM_CLOUD_ENUM_URL: &str =
    "https://api.steampowered.com/ICloudService/EnumerateUserFiles/v1/";
const STORE_SESSIONS_FILE: &str = "store_sessions.json";

const EMSG_PROTO_MASK: u32 = 0x8000_0000;
const EMSG_CLIENT_HEARTBEAT: u32 = 703;
const EMSG_CLIENT_LOGON: u32 = 5514;
const EMSG_CLIENT_CHAT_MSG: u32 = 104;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CmServerEndpoint {
    pub endpoint: String,
    pub server_type: String,
    pub load: f64,
    pub weighted_load: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamJwtClaims {
    pub steam_id: String,
    pub issuer: String,
    pub expires_at: u64,
    pub audience: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamCmSessionStatus {
    pub connected: bool,
    pub connecting: bool,
    pub steam_id: String,
    pub persona_name: String,
    pub active_cm_endpoint: Option<String>,
    pub cm_pool_count: usize,
    pub token_expires_at: u64,
    pub reconnect_count: u64,
    pub last_heartbeat_ts: u64,
    pub last_error: Option<String>,
    pub pending_invites: usize,
    pub chat_message_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamChatMessage {
    pub id: String,
    pub peer_steam_id: String,
    pub peer_name: String,
    pub sender_is_self: bool,
    pub message: String,
    pub timestamp_secs: u64,
    pub delivery_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamGameInvite {
    pub invite_id: String,
    pub sender_steam_id: String,
    pub sender_name: String,
    pub app_id: String,
    pub game_name: String,
    pub lobby_steam_id: Option<String>,
    pub connect_string: Option<String>,
    pub received_at_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamCloudFileEntry {
    pub filename: String,
    pub app_id: u32,
    pub file_size: u64,
    pub timestamp_secs: u64,
    pub sha_hex: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamCloudSyncReport {
    pub app_id: u32,
    pub target_dir: String,
    pub remote_file_count: usize,
    pub downloaded_count: usize,
    pub bytes_synced: u64,
    pub summary: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedStoreSessions {
    pub steam_refresh_token: String,
    pub steam_access_token: String,
    pub steam_id: String,
    pub steam_persona_name: String,
    pub gog_refresh_token: String,
    pub gog_access_token: String,
    pub gog_user_id: String,
    pub epic_refresh_token: String,
    pub epic_access_token: String,
    pub epic_account_id: String,
    pub epic_display_name: String,
}

#[derive(Default)]
struct CmRuntimeState {
    connected: bool,
    connecting: bool,
    active_cm_endpoint: Option<String>,
    cm_pool_count: usize,
    token_expires_at: u64,
    last_error: Option<String>,
    shutdown_tx: Option<mpsc::Sender<()>>,
    outgoing_tx: Option<mpsc::UnboundedSender<Vec<u8>>>,
}

static CM_STATE: OnceLock<Mutex<CmRuntimeState>> = OnceLock::new();
static CM_CONNECTED_FLAG: AtomicBool = AtomicBool::new(false);
static CM_RECONNECT_COUNT: AtomicU64 = AtomicU64::new(0);
static CM_LAST_HEARTBEAT_TS: AtomicU64 = AtomicU64::new(0);
static STEAM_CHAT_LOG: OnceLock<Mutex<Vec<SteamChatMessage>>> = OnceLock::new();
static STEAM_INVITES: OnceLock<Mutex<Vec<SteamGameInvite>>> = OnceLock::new();
static STORE_SESSIONS_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn cm_state() -> &'static Mutex<CmRuntimeState> {
    CM_STATE.get_or_init(|| Mutex::new(CmRuntimeState::default()))
}

fn chat_log() -> &'static Mutex<Vec<SteamChatMessage>> {
    STEAM_CHAT_LOG.get_or_init(|| Mutex::new(Vec::new()))
}

fn pending_invites() -> &'static Mutex<Vec<SteamGameInvite>> {
    STEAM_INVITES.get_or_init(|| Mutex::new(Vec::new()))
}

fn sessions_io_lock() -> &'static Mutex<()> {
    STORE_SESSIONS_IO_LOCK.get_or_init(|| Mutex::new(()))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn load_persisted_store_sessions() -> PersistedStoreSessions {
    let _g = sessions_io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let Ok(path) = crate::get_app_data_file_standalone(STORE_SESSIONS_FILE) else {
        return PersistedStoreSessions::default();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_persisted_store_sessions(sessions: &PersistedStoreSessions) -> Result<(), String> {
    let _g = sessions_io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let path = crate::get_app_data_file_standalone(STORE_SESSIONS_FILE)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed creating store session dir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(sessions)
        .map_err(|e| format!("Failed serializing store sessions: {e}"))?;
    fs::write(&path, json).map_err(|e| format!("Failed saving store sessions: {e}"))
}

/// Decodes Steam JWT claims (`header.payload.sig`) without external JWT crate overhead.
pub fn decode_steam_jwt_claims(token: &str) -> Result<SteamJwtClaims, String> {
    let mut parts = token.trim().split('.');
    let _hdr = parts
        .next()
        .ok_or_else(|| "Invalid JWT format: missing header".to_string())?;
    let payload_b64 = parts
        .next()
        .ok_or_else(|| "Invalid JWT format: missing payload".to_string())?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(payload_b64))
        .map_err(|e| format!("Failed base64-decoding Steam JWT payload: {e}"))?;
    let val: Value = serde_json::from_slice(&decoded)
        .map_err(|e| format!("Invalid Steam JWT JSON payload: {e}"))?;
    let steam_id = val
        .get("sub")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let issuer = val
        .get("iss")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let expires_at = val.get("exp").and_then(|v| v.as_u64()).unwrap_or(0);
    let audience = val
        .get("aud")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Ok(SteamJwtClaims {
        steam_id,
        issuer,
        expires_at,
        audience,
    })
}

/// Parses `ISteamDirectory/GetCMListForConnect/v1/` response JSON and sorts WebSocket servers by load.
pub fn parse_cm_server_list_json(json_str: &str) -> Result<Vec<CmServerEndpoint>, String> {
    let v: Value = serde_json::from_str(json_str)
        .map_err(|e| format!("Invalid Steam GetCMListForConnect JSON: {e}"))?;
    let arr = v
        .get("response")
        .and_then(|r| r.get("serverlist"))
        .and_then(|s| s.as_array())
        .ok_or_else(|| "Missing response.serverlist in Steam CM response".to_string())?;

    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let Some(endpoint) = item
            .get("endpoint")
            .and_then(|e| e.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let server_type = item
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("websockets")
            .to_string();
        if !server_type.eq_ignore_ascii_case("websockets") {
            continue;
        }
        let load = item.get("load").and_then(|n| n.as_f64()).unwrap_or(100.0);
        let weighted_load = item
            .get("wtd_load")
            .and_then(|n| n.as_f64())
            .unwrap_or(load);
        out.push(CmServerEndpoint {
            endpoint: endpoint.to_string(),
            server_type,
            load,
            weighted_load,
        });
    }
    out.sort_by(|a, b| {
        a.weighted_load
            .partial_cmp(&b.weighted_load)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

/// Encodes a minimal Valve CM protobuf-masked packet header + protobuf payload.
pub fn encode_cm_proto_packet(emsg_raw: u32, proto_header: &[u8], body: &[u8]) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(8 + proto_header.len() + body.len());
    pkt.extend_from_slice(&(emsg_raw | EMSG_PROTO_MASK).to_le_bytes());
    pkt.extend_from_slice(&(proto_header.len() as u32).to_le_bytes());
    pkt.extend_from_slice(proto_header);
    pkt.extend_from_slice(body);
    pkt
}

/// Decodes a Valve CM protobuf-masked packet returning `(emsg_id, proto_header, body)`.
pub fn decode_cm_proto_packet(packet: &[u8]) -> Result<(u32, &[u8], &[u8]), String> {
    if packet.len() < 8 {
        return Err("Truncated Steam CM frame (< 8 bytes)".to_string());
    }
    let raw_emsg = u32::from_le_bytes([packet[0], packet[1], packet[2], packet[3]]);
    let hdr_len = u32::from_le_bytes([packet[4], packet[5], packet[6], packet[7]]) as usize;
    if 8 + hdr_len > packet.len() {
        return Err("Steam CM header length exceeds frame size".to_string());
    }
    Ok((
        raw_emsg & !EMSG_PROTO_MASK,
        &packet[8..8 + hdr_len],
        &packet[8 + hdr_len..],
    ))
}

fn build_cm_client_logon_proto_header(steam_id_u64: u64) -> Vec<u8> {
    // Field 1 (fixed64 steamid, wire type 1 => tag 0x09)
    let mut hdr = Vec::with_capacity(12);
    hdr.push(0x09);
    hdr.extend_from_slice(&steam_id_u64.to_le_bytes());
    hdr
}

pub fn parse_steam_cloud_file_list_json(app_id: u32, json_str: &str) -> Vec<SteamCloudFileEntry> {
    let Ok(v) = serde_json::from_str::<Value>(json_str) else {
        return Vec::new();
    };
    let Some(files) = v
        .get("response")
        .and_then(|r| r.get("files"))
        .and_then(|f| f.as_array())
    else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(files.len());
    for item in files {
        let Some(filename) = item
            .get("filename")
            .and_then(|s| s.as_str())
            .filter(|s| !s.trim().is_empty())
        else {
            continue;
        };
        out.push(SteamCloudFileEntry {
            filename: filename.to_string(),
            app_id,
            file_size: item.get("file_size").and_then(|n| n.as_u64()).unwrap_or(0),
            timestamp_secs: item.get("timestamp").and_then(|n| n.as_u64()).unwrap_or(0),
            sha_hex: item
                .get("file_sha")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            url: item
                .get("url")
                .and_then(|s| s.as_str())
                .map(str::to_string),
        });
    }
    out
}

pub fn parse_steam_invite_payload(val: &Value) -> Option<SteamGameInvite> {
    let sender_steam_id = val
        .get("senderSteamId")
        .or_else(|| val.get("steamid_sender"))
        .and_then(|v| v.as_str())?
        .to_string();
    let sender_name = val
        .get("senderName")
        .or_else(|| val.get("persona_name"))
        .and_then(|v| v.as_str())
        .unwrap_or("Steam Friend")
        .to_string();
    let app_id = val
        .get("appId")
        .or_else(|| val.get("appid"))
        .and_then(|v| {
            v.as_str()
                .map(str::to_string)
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        })
        .unwrap_or_else(|| "0".to_string());
    let game_name = val
        .get("gameName")
        .or_else(|| val.get("game_name"))
        .and_then(|v| v.as_str())
        .unwrap_or("Multiplayer Session")
        .to_string();
    let lobby_steam_id = val
        .get("lobbySteamId")
        .or_else(|| val.get("lobby_id"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let connect_string = val
        .get("connect")
        .or_else(|| val.get("connectString"))
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let invite_id = format!("{}_{}_{}", sender_steam_id, app_id, now_secs());
    Some(SteamGameInvite {
        invite_id,
        sender_steam_id,
        sender_name,
        app_id,
        game_name,
        lobby_steam_id,
        connect_string,
        received_at_secs: now_secs(),
    })
}

async fn refresh_steam_access_token_from_cm(refresh_token: &str, steam_id: &str) -> Result<String, String> {
    if refresh_token.trim().is_empty() {
        return Err("Steam refresh token is not configured.".to_string());
    }
    let client = duke_core::net_doh::get_configured_http_client();
    let mut form = HashMap::new();
    form.insert("refresh_token", refresh_token.trim());
    form.insert("steamid", steam_id.trim());
    let resp = client
        .post(STEAM_GEN_TOKEN_URL)
        .form(&form)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Steam GenerateAccessTokenForApp failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Steam token exchange HTTP {}", resp.status()));
    }
    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("Invalid Steam token JSON: {e}"))?;
    body.get("response")
        .and_then(|r| r.get("access_token"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Steam token exchange did not return access_token".to_string())
}

fn current_status_snapshot() -> SteamCmSessionStatus {
    let persisted = load_persisted_store_sessions();
    let state = cm_state().lock().unwrap_or_else(|e| e.into_inner());
    let invites = pending_invites().lock().map(|v| v.len()).unwrap_or(0);
    let msgs = chat_log().lock().map(|v| v.len()).unwrap_or(0);
    SteamCmSessionStatus {
        connected: CM_CONNECTED_FLAG.load(Ordering::Relaxed) || state.connected,
        connecting: state.connecting,
        steam_id: persisted.steam_id,
        persona_name: persisted.steam_persona_name,
        active_cm_endpoint: state.active_cm_endpoint.clone(),
        cm_pool_count: state.cm_pool_count,
        token_expires_at: state.token_expires_at,
        reconnect_count: CM_RECONNECT_COUNT.load(Ordering::Relaxed),
        last_heartbeat_ts: CM_LAST_HEARTBEAT_TS.load(Ordering::Relaxed),
        last_error: state.last_error.clone(),
        pending_invites: invites,
        chat_message_count: msgs,
    }
}

#[tauri::command]
pub async fn get_steam_cm_status() -> SteamCmSessionStatus {
    current_status_snapshot()
}

#[tauri::command]
pub async fn connect_steam_cm(
    app: AppHandle,
    refresh_token: Option<String>,
    persona_name: Option<String>,
) -> Result<SteamCmSessionStatus, String> {
    let mut persisted = load_persisted_store_sessions();
    if let Some(ref tok) = refresh_token {
        let trimmed = tok.trim();
        if !trimmed.is_empty() {
            persisted.steam_refresh_token = trimmed.to_string();
        }
    }
    if let Some(ref name) = persona_name {
        if !name.trim().is_empty() {
            persisted.steam_persona_name = name.trim().to_string();
        }
    }

    let claims = if !persisted.steam_refresh_token.is_empty() {
        decode_steam_jwt_claims(&persisted.steam_refresh_token).unwrap_or_default()
    } else {
        SteamJwtClaims::default()
    };
    if !claims.steam_id.is_empty() {
        persisted.steam_id = claims.steam_id.clone();
    } else if persisted.steam_id.is_empty() {
        if let Some(detected) = crate::friends::detect_local_steam_id64() {
            persisted.steam_id = detected;
        }
    }
    save_persisted_store_sessions(&persisted)?;

    // Stop any previous CM WebSocket worker cleanly before starting a fresh session.
    let prev_tx = cm_state()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .shutdown_tx
        .take();
    if let Some(tx) = prev_tx {
        let _ = tx.send(()).await;
    }

    let client = duke_core::net_doh::get_configured_http_client();
    let cm_resp = client
        .get(STEAM_DIR_CM_URL)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Failed querying Steam CM Directory: {e}"))?;
    let cm_json = cm_resp
        .text()
        .await
        .map_err(|e| format!("Failed reading Steam CM Directory body: {e}"))?;
    let endpoints = parse_cm_server_list_json(&cm_json)?;
    if endpoints.is_empty() {
        return Err("Steam CM directory returned zero WebSocket servers.".to_string());
    }

    let primary = endpoints[0].endpoint.clone();
    let wss_url = format!("wss://{}/cmsocket/", primary);

    // Attempt initial access-token exchange if refresh token is a JWT.
    if !persisted.steam_refresh_token.is_empty() && !persisted.steam_id.is_empty() {
        if let Ok(access) =
            refresh_steam_access_token_from_cm(&persisted.steam_refresh_token, &persisted.steam_id)
                .await
        {
            persisted.steam_access_token = access;
            let _ = save_persisted_store_sessions(&persisted);
        }
    }

    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    {
        let mut s = cm_state().lock().unwrap_or_else(|e| e.into_inner());
        s.connecting = true;
        s.connected = false;
        s.active_cm_endpoint = Some(primary.clone());
        s.cm_pool_count = endpoints.len();
        s.token_expires_at = claims.expires_at;
        s.last_error = None;
        s.shutdown_tx = Some(shutdown_tx);
        s.outgoing_tx = Some(out_tx);
    }

    let (ws_stream, _) = tokio_tungstenite::connect_async(&wss_url)
        .await
        .map_err(|e| {
            let msg = format!("Steam CM WebSocket handshake failed ({wss_url}): {e}");
            if let Ok(mut s) = cm_state().lock() {
                s.connecting = false;
                s.connected = false;
                s.last_error = Some(msg.clone());
            }
            CM_CONNECTED_FLAG.store(false, Ordering::Relaxed);
            msg
        })?;

    CM_CONNECTED_FLAG.store(true, Ordering::Relaxed);
    CM_LAST_HEARTBEAT_TS.store(now_secs(), Ordering::Relaxed);
    if let Ok(mut s) = cm_state().lock() {
        s.connecting = false;
        s.connected = true;
        s.last_error = None;
    }

    let steam_id_num = persisted.steam_id.parse::<u64>().unwrap_or(0);
    let app_ev = app.clone();
    let cm_hosts: Vec<String> = endpoints.into_iter().take(6).map(|e| e.endpoint).collect();

    tokio::spawn(async move {
        let (mut ws_write, mut ws_read) = ws_stream.split();
        let hdr = build_cm_client_logon_proto_header(steam_id_num);
        let logon_pkt = encode_cm_proto_packet(EMSG_CLIENT_LOGON, &hdr, &[]);
        let _ = ws_write.send(Message::Binary(logon_pkt.into())).await;

        let mut heartbeat = tokio::time::interval(Duration::from_secs(20));
        let _ = heartbeat.tick().await;
        crate::emit_if_alive(&app_ev, "steam-cm-state", current_status_snapshot());

        loop {
            if crate::is_shutting_down() {
                let _ = ws_write.send(Message::Close(None)).await;
                break;
            }
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    let _ = ws_write.send(Message::Close(None)).await;
                    break;
                }
                Some(raw_out) = out_rx.recv() => {
                    if ws_write.send(Message::Binary(raw_out.into())).await.is_err() {
                        break;
                    }
                }
                _ = heartbeat.tick() => {
                    if crate::is_shutting_down() {
                        let _ = ws_write.send(Message::Close(None)).await;
                        break;
                    }
                    let hb = encode_cm_proto_packet(EMSG_CLIENT_HEARTBEAT, &hdr, &[]);
                    if ws_write.send(Message::Binary(hb.into())).await.is_err() {
                        CM_RECONNECT_COUNT.fetch_add(1, Ordering::Relaxed);
                        if let Some(next_host) = cm_hosts.get(1) {
                            let retry_url = format!("wss://{next_host}/cmsocket/");
                            if let Ok((new_ws, _)) = tokio_tungstenite::connect_async(&retry_url).await {
                                let (nw, nr) = new_ws.split();
                                ws_write = nw;
                                ws_read = nr;
                                if let Ok(mut s) = cm_state().lock() {
                                    s.active_cm_endpoint = Some(next_host.clone());
                                }
                                continue;
                            }
                        }
                        break;
                    }
                    CM_LAST_HEARTBEAT_TS.store(now_secs(), Ordering::Relaxed);
                }
                msg_opt = ws_read.next() => {
                    match msg_opt {
                        Some(Ok(Message::Binary(bin))) => {
                            if let Ok((emsg, _, body)) = decode_cm_proto_packet(&bin) {
                                if emsg == EMSG_CLIENT_CHAT_MSG {
                                    if let Ok(val) = serde_json::from_slice::<Value>(body) {
                                        if let Some(inv) = parse_steam_invite_payload(&val) {
                                            if let Ok(mut g) = pending_invites().lock() {
                                                g.push(inv.clone());
                                            }
                                            crate::emit_if_alive(&app_ev, "steam-game-invite", &inv);
                                        }
                                    }
                                }
                            }
                        }
                        Some(Ok(Message::Text(txt))) => {
                            if let Ok(val) = serde_json::from_str::<Value>(&txt) {
                                if let Some(inv) = parse_steam_invite_payload(&val) {
                                    if let Ok(mut g) = pending_invites().lock() {
                                        g.push(inv.clone());
                                    }
                                    crate::emit_if_alive(&app_ev, "steam-game-invite", &inv);
                                }
                            }
                        }
                        Some(Ok(Message::Ping(p))) => {
                            let _ = ws_write.send(Message::Pong(p)).await;
                        }
                        Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }

        CM_CONNECTED_FLAG.store(false, Ordering::Relaxed);
        if let Ok(mut s) = cm_state().lock() {
            s.connected = false;
            s.connecting = false;
            s.outgoing_tx = None;
        }
        crate::emit_if_alive(&app_ev, "steam-cm-state", current_status_snapshot());
    });

    Ok(current_status_snapshot())
}

#[tauri::command]
pub async fn disconnect_steam_cm(app: AppHandle) -> SteamCmSessionStatus {
    let tx = cm_state()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .shutdown_tx
        .take();
    if let Some(s) = tx {
        let _ = s.send(()).await;
    }
    CM_CONNECTED_FLAG.store(false, Ordering::Relaxed);
    {
        let mut st = cm_state().lock().unwrap_or_else(|e| e.into_inner());
        st.connected = false;
        st.connecting = false;
        st.outgoing_tx = None;
    }
    let snap = current_status_snapshot();
    crate::emit_if_alive(&app, "steam-cm-state", &snap);
    snap
}

#[tauri::command]
pub async fn send_steam_chat_message(
    app: AppHandle,
    peer_steam_id: String,
    peer_name: Option<String>,
    message: String,
) -> Result<SteamChatMessage, String> {
    let target = peer_steam_id.trim().to_string();
    let body = message.trim().to_string();
    if target.is_empty() || body.is_empty() {
        return Err("Target SteamID and message text are required.".to_string());
    }
    let persisted = load_persisted_store_sessions();
    let delivery_status: String;

    if !persisted.steam_access_token.is_empty() {
        let client = duke_core::net_doh::get_configured_http_client();
        let mut form = HashMap::new();
        form.insert("access_token", persisted.steam_access_token.as_str());
        form.insert("steamid", target.as_str());
        form.insert("chat_entry_type", "1");
        form.insert("message", body.as_str());
        match client
            .post(STEAM_SEND_CHAT_URL)
            .form(&form)
            .timeout(Duration::from_secs(8))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                delivery_status = "sent".to_string();
            }
            Ok(resp) => {
                delivery_status = format!("http-{}", resp.status().as_u16());
            }
            Err(e) => {
                delivery_status = format!("transport-error: {e}");
            }
        }
    } else if let Some(tx) = cm_state()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .outgoing_tx
        .clone()
    {
        let payload = serde_json::json!({
            "targetSteamId": target,
            "message": body,
        })
        .to_string()
        .into_bytes();
        let pkt = encode_cm_proto_packet(EMSG_CLIENT_CHAT_MSG, &[], &payload);
        let _ = tx.send(pkt);
        delivery_status = "cm-ws-sent".to_string();
    } else {
        return Err("Steam CM session is not authenticated. Connect with a Steam refresh token first.".to_string());
    }

    let entry = SteamChatMessage {
        id: format!("msg_{}_{}", target, now_secs()),
        peer_steam_id: target,
        peer_name: peer_name.unwrap_or_else(|| "Steam Friend".to_string()),
        sender_is_self: true,
        message: body,
        timestamp_secs: now_secs(),
        delivery_status,
    };
    if let Ok(mut g) = chat_log().lock() {
        g.push(entry.clone());
        if g.len() > 200 {
            let excess = g.len() - 200;
            g.drain(..excess);
        }
    }
    crate::emit_if_alive(&app, "steam-chat-message", &entry);
    Ok(entry)
}

#[tauri::command]
pub async fn get_steam_chat_messages(peer_steam_id: Option<String>) -> Vec<SteamChatMessage> {
    let filter = peer_steam_id.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    chat_log()
        .lock()
        .map(|g| {
            g.iter()
                .filter(|m| filter.as_ref().map(|p| m.peer_steam_id == *p).unwrap_or(true))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

#[tauri::command]
pub async fn get_steam_pending_invites() -> Vec<SteamGameInvite> {
    pending_invites()
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn respond_steam_game_invite(
    app: AppHandle,
    invite_id: String,
    accept: bool,
) -> Result<Option<String>, String> {
    let invite_opt = {
        let mut g = pending_invites()
            .lock()
            .map_err(|_| "Invites lock poisoned".to_string())?;
        g.iter()
            .position(|i| i.invite_id == invite_id)
            .map(|idx| g.remove(idx))
    };
    let Some(invite) = invite_opt else {
        return Ok(None);
    };
    crate::emit_if_alive(&app, "steam-invites-updated", get_steam_pending_invites().await);
    if !accept {
        return Ok(None);
    }
    let uri = if let Some(ref lobby) = invite.lobby_steam_id {
        format!(
            "steam://joinlobby/{}/{}/{}",
            invite.app_id, lobby, invite.sender_steam_id
        )
    } else if let Some(ref conn) = invite.connect_string {
        format!("steam://connect/{}", conn.trim())
    } else {
        format!("steam://run/{}", invite.app_id)
    };
    open::that(&uri).map_err(|e| format!("Failed launching invite join URI: {e}"))?;
    Ok(Some(uri))
}

#[tauri::command]
pub async fn list_steam_cloud_files(app_id: u32) -> Result<Vec<SteamCloudFileEntry>, String> {
    let persisted = load_persisted_store_sessions();
    let token = if !persisted.steam_access_token.is_empty() {
        persisted.steam_access_token.clone()
    } else if !persisted.steam_refresh_token.is_empty() && !persisted.steam_id.is_empty() {
        refresh_steam_access_token_from_cm(&persisted.steam_refresh_token, &persisted.steam_id)
            .await?
    } else {
        return Err("Steam CM refresh/access token required to enumerate Steam Cloud files.".to_string());
    };
    let client = duke_core::net_doh::get_configured_http_client();
    let resp = client
        .get(STEAM_CLOUD_ENUM_URL)
        .query(&[
            ("access_token", token.as_str()),
            ("appid", &app_id.to_string()),
            ("extended_details", "true"),
        ])
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Steam Cloud EnumerateUserFiles failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Steam Cloud HTTP {}", resp.status()));
    }
    let text = resp
        .text()
        .await
        .map_err(|e| format!("Failed reading Steam Cloud response: {e}"))?;
    Ok(parse_steam_cloud_file_list_json(app_id, &text))
}

#[tauri::command]
pub async fn sync_steam_cloud_saves(
    app: AppHandle,
    game_id: String,
    app_id: u32,
) -> Result<SteamCloudSyncReport, String> {
    let files = list_steam_cloud_files(app_id).await?;
    let lib = crate::load_library_lightweight(&app)?;
    let entry = lib
        .iter()
        .find(|g| g.id == game_id)
        .ok_or_else(|| format!("Game '{}' not found in library.", game_id))?;

    let target_dir = PathBuf::from(&entry.install_dir).join(".duke-profile").join("SteamCloud");
    fs::create_dir_all(&target_dir)
        .map_err(|e| format!("Failed creating Steam Cloud sync folder: {e}"))?;

    let client = duke_core::net_doh::get_configured_http_client();
    let mut downloaded_count = 0usize;
    let mut bytes_synced = 0u64;

    for f in &files {
        let Some(ref url) = f.url else {
            continue;
        };
        let clean_name = f.filename.replace(['\\', ':'], "/");
        let mut dest = target_dir.clone();
        for seg in clean_name.split('/') {
            if seg.is_empty() || seg == "." || seg == ".." {
                continue;
            }
            dest.push(seg);
        }
        if let Some(p) = dest.parent() {
            let _ = fs::create_dir_all(p);
        }
        if let Ok(resp) = client.get(url).timeout(Duration::from_secs(20)).send().await {
            if resp.status().is_success() {
                if let Ok(bytes) = resp.bytes().await {
                    if fs::write(&dest, &bytes).is_ok() {
                        downloaded_count += 1;
                        bytes_synced = bytes_synced.saturating_add(bytes.len() as u64);
                    }
                }
            }
        }
    }

    Ok(SteamCloudSyncReport {
        app_id,
        target_dir: target_dir.to_string_lossy().into_owned(),
        remote_file_count: files.len(),
        downloaded_count,
        bytes_synced,
        summary: format!(
            "Synced {}/{} Steam Cloud save file(s) ({}) into .duke-profile/SteamCloud without steamwebhelper.",
            downloaded_count,
            files.len(),
            crate::format_bytes(bytes_synced)
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_jwt_claims_and_cm_server_list_parse_cleanly() {
        let payload = serde_json::json!({
            "sub": "76561198000000001",
            "iss": "steam",
            "exp": 1999999999u64,
            "aud": ["client", "web"]
        });
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(payload.to_string().as_bytes());
        let token = format!("eyJhbGciOiJFZERTQSJ9.{b64}.sig");
        let claims = decode_steam_jwt_claims(&token).unwrap();
        assert_eq!(claims.steam_id, "76561198000000001");
        assert_eq!(claims.expires_at, 1999999999);
        assert_eq!(claims.audience, vec!["client".to_string(), "web".to_string()]);

        let cm_json = r#"{
            "response": {
                "serverlist": [
                    { "endpoint": "ext1-iad1.steamserver.net:27021", "type": "netfilter", "load": 5.0 },
                    { "endpoint": "ext2-ord1.steamserver.net:443", "type": "websockets", "load": 42.0, "wtd_load": 40.0 },
                    { "endpoint": "ext1-iad1.steamserver.net:443", "type": "websockets", "load": 15.0, "wtd_load": 12.5 }
                ]
            }
        }"#;
        let eps = parse_cm_server_list_json(cm_json).unwrap();
        assert_eq!(eps.len(), 2);
        assert_eq!(eps[0].endpoint, "ext1-iad1.steamserver.net:443");
    }

    #[test]
    fn steam_cm_packet_codec_and_cloud_json_work() {
        let pkt = encode_cm_proto_packet(EMSG_CLIENT_LOGON, &[0x09, 1, 2, 3], b"payload");
        let (emsg, hdr, body) = decode_cm_proto_packet(&pkt).unwrap();
        assert_eq!(emsg, EMSG_CLIENT_LOGON);
        assert_eq!(hdr, &[0x09, 1, 2, 3]);
        assert_eq!(body, b"payload");

        let cloud_json = r#"{
            "response": {
                "files": [
                    { "filename": "slot1.sav", "file_size": 8192, "timestamp": 1700000000, "file_sha": "deadbeef", "url": "https://cdn.example/slot1.sav" }
                ]
            }
        }"#;
        let entries = parse_steam_cloud_file_list_json(413150, cloud_json);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].filename, "slot1.sav");
        assert_eq!(entries[0].file_size, 8192);
    }
}
