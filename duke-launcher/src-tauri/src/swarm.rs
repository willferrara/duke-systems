use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

use crate::delta_patch::{
    apply_delta_with_local_stitching, build_dkmanifest_from_dir, create_zstd_delta_bundle,
    diff_local_dir_against_manifest, safe_join_rel_path, DkManifest,
};
use crate::{
    atomic_write_string, calculate_dir_size_sync, clean_game_title, find_best_game_exe_in_folder,
    format_bytes, get_app_data_file_standalone, load_library_standalone, load_settings_standalone,
    resolve_game_root_dir, sanitize_filename, save_library_standalone, GameEntry,
};

pub const SWARM_MULTICAST_V4: Ipv4Addr = Ipv4Addr::new(239, 255, 77, 88);
pub const SWARM_DISCOVERY_PORT: u16 = 47880;
pub const SWARM_DEFAULT_TCP_PORT: u16 = 47881;
const SWARM_MAGIC: &str = "DKSWRM1";
const MAX_FRAME_BYTES: usize = 32 * 1024 * 1024;

pub type VectorClock = BTreeMap<String, u64>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClockOrdering {
    Equal,
    Before,
    After,
    Concurrent,
}

pub fn compare_vector_clocks(local: &VectorClock, remote: &VectorClock) -> ClockOrdering {
    let mut keys = HashSet::new();
    for k in local.keys() {
        keys.insert(k.as_str());
    }
    for k in remote.keys() {
        keys.insert(k.as_str());
    }

    let mut local_gt = false;
    let mut remote_gt = false;
    for k in keys {
        let l = local.get(k).copied().unwrap_or(0);
        let r = remote.get(k).copied().unwrap_or(0);
        if l > r {
            local_gt = true;
        } else if r > l {
            remote_gt = true;
        }
    }

    match (local_gt, remote_gt) {
        (false, false) => ClockOrdering::Equal,
        (false, true) => ClockOrdering::Before,
        (true, false) => ClockOrdering::After,
        (true, true) => ClockOrdering::Concurrent,
    }
}

pub fn merge_vector_clocks(local: &mut VectorClock, remote: &VectorClock) {
    for (k, &v) in remote {
        let slot = local.entry(k.clone()).or_insert(0);
        if v > *slot {
            *slot = v;
        }
    }
}

pub fn increment_vector_clock(clock: &mut VectorClock, node_id: &str) {
    let slot = clock.entry(node_id.to_string()).or_insert(0);
    *slot = slot.saturating_add(1);
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SwarmAppAd {
    pub game_key: String,
    pub local_id: String,
    pub title: String,
    pub app_id: Option<u32>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwarmBeacon {
    pub magic: String,
    pub node_id: String,
    pub node_name: String,
    pub tcp_port: u16,
    pub apps: Vec<SwarmAppAd>,
    pub save_clocks: BTreeMap<String, VectorClock>,
    pub timestamp_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwarmPeerInfo {
    pub node_id: String,
    pub node_name: String,
    pub addr: String,
    pub tcp_port: u16,
    pub transport: String,
    pub last_seen_secs: u64,
    pub apps: Vec<SwarmAppAd>,
    pub save_clocks: BTreeMap<String, VectorClock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwarmStatusSnapshot {
    pub enabled: bool,
    pub node_id: String,
    pub node_name: String,
    pub tcp_port: u16,
    pub multicast_group: String,
    pub configured_peers: Vec<String>,
    pub peers: Vec<SwarmPeerInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeamTransferReport {
    pub peer_name: String,
    pub game_title: String,
    pub dest_dir: String,
    pub files_total: usize,
    pub total_bytes: u64,
    pub downloaded_delta_bytes: u64,
    pub reused_bytes: u64,
    pub throughput_mbps: f64,
    pub elapsed_ms: u64,
    pub registered_game_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct MeshSaveSyncReport {
    pub game_id: String,
    pub game_title: String,
    pub save_key: String,
    pub local_clock: VectorClock,
    pub peers_checked: usize,
    pub peers_synced: usize,
    pub bytes_pushed: u64,
    pub bytes_pulled: u64,
    pub conflict_backup_created: bool,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct SwarmLocalState {
    node_id: String,
    save_clocks: BTreeMap<String, VectorClock>,
    save_digests: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub(crate) enum SwarmWireRequest {
    GetBeacon,
    BeamManifest {
        game_key: String,
    },
    BeamDeltaBundle {
        game_key: String,
        missing_hashes: Vec<String>,
    },
    SaveSyncOffer {
        save_key: String,
        sender_clock: VectorClock,
        manifest: Option<DkManifest>,
        delta_bytes: Option<Vec<u8>>,
    },
    SaveSyncPull {
        save_key: String,
        have_hashes: Vec<String>,
    },
    ShaderPull {
        game_blake3: String,
        gpu_key: String,
        driver_version: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub(crate) enum SwarmWireResponse {
    Beacon(SwarmBeacon),
    Manifest(DkManifest),
    DeltaBundle {
        bundle_bytes: Vec<u8>,
    },
    SaveState {
        clock: VectorClock,
        manifest: Option<DkManifest>,
        bundle_bytes: Option<Vec<u8>>,
        applied: bool,
        conflict_backup: bool,
    },
    ShaderBundle {
        found: bool,
        bundle_zst_bytes: Vec<u8>,
        key: String,
        file_count: usize,
    },
    Error {
        message: String,
    },
}

static SWARM_PEERS: OnceLock<Mutex<HashMap<String, SwarmPeerInfo>>> = OnceLock::new();
static SWARM_LISTENING_PORT: OnceLock<Mutex<u16>> = OnceLock::new();
static SWARM_STARTED: OnceLock<()> = OnceLock::new();
static SWARM_STATE_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn peer_registry() -> &'static Mutex<HashMap<String, SwarmPeerInfo>> {
    SWARM_PEERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn listening_port_slot() -> &'static Mutex<u16> {
    SWARM_LISTENING_PORT.get_or_init(|| Mutex::new(0))
}

fn state_io_lock() -> &'static Mutex<()> {
    SWARM_STATE_IO_LOCK.get_or_init(|| Mutex::new(()))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn load_or_init_swarm_state() -> SwarmLocalState {
    let _g = state_io_lock().lock().unwrap();
    let Ok(path) = get_app_data_file_standalone("swarm_state.json") else {
        return SwarmLocalState {
            node_id: format!("node-{}", std::process::id()),
            ..SwarmLocalState::default()
        };
    };
    if let Ok(raw) = fs::read_to_string(&path) {
        if let Ok(mut parsed) = serde_json::from_str::<SwarmLocalState>(&raw) {
            if !parsed.node_id.trim().is_empty() {
                return parsed;
            }
            parsed.node_id = format!("node-{}", now_secs());
            let _ = save_swarm_state_locked(&path, &parsed);
            return parsed;
        }
    }
    let host_seed = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "duke-node".to_string());
    let digest = blake3::hash(format!("{}:{}", host_seed, now_secs()).as_bytes())
        .to_hex()
        .to_string();
    let state = SwarmLocalState {
        node_id: format!("duke-{}", &digest[..12]),
        save_clocks: BTreeMap::new(),
        save_digests: BTreeMap::new(),
    };
    let _ = save_swarm_state_locked(&path, &state);
    state
}

fn save_swarm_state_locked(path: &Path, state: &SwarmLocalState) -> Result<(), String> {
    let serialized = serde_json::to_string_pretty(state)
        .map_err(|e| format!("Failed serializing swarm state: {}", e))?;
    atomic_write_string(path, &serialized)
}

fn save_swarm_state(state: &SwarmLocalState) -> Result<(), String> {
    let _g = state_io_lock().lock().unwrap();
    let path = get_app_data_file_standalone("swarm_state.json")?;
    save_swarm_state_locked(&path, state)
}

pub fn local_node_identity() -> (String, String) {
    let state = load_or_init_swarm_state();
    let settings = load_settings_standalone();
    let custom_name = settings.swarm_node_name.trim().to_string();
    let node_name = if !custom_name.is_empty() {
        custom_name
    } else {
        std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| state.node_id.clone())
    };
    (state.node_id, node_name)
}

fn game_key_for_entry(entry: &GameEntry) -> String {
    if let Some(app_id) = entry.steam_app_id.or_else(|| entry.app_id.map(u64::from)) {
        format!("steam:{}", app_id)
    } else {
        let cleaned = clean_game_title(&entry.title).to_ascii_lowercase();
        let slug: String = cleaned
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        format!("title:{}", if slug.is_empty() { &entry.id } else { &slug })
    }
}

pub fn build_local_beacon(tcp_port: u16) -> SwarmBeacon {
    let state = load_or_init_swarm_state();
    let (_, node_name) = local_node_identity();
    let library = load_library_standalone().unwrap_or_default();
    let apps = library
        .into_iter()
        .filter(|g| !g.install_dir.trim().is_empty() || !g.exe_path.trim().is_empty())
        .map(|g| SwarmAppAd {
            game_key: game_key_for_entry(&g),
            local_id: g.id.clone(),
            title: g.title,
            app_id: g.app_id.or_else(|| g.steam_app_id.and_then(|v| u32::try_from(v).ok())),
            size_bytes: g.size_bytes,
        })
        .collect();

    SwarmBeacon {
        magic: SWARM_MAGIC.to_string(),
        node_id: state.node_id,
        node_name,
        tcp_port,
        apps,
        save_clocks: state.save_clocks,
        timestamp_secs: now_secs(),
    }
}

fn resolve_game_dir_by_key(game_key: &str) -> Result<PathBuf, String> {
    let library = load_library_standalone()?;
    let matched = library.into_iter().find(|g| {
        g.id == game_key
            || game_key_for_entry(g) == game_key
            || g.title.eq_ignore_ascii_case(game_key)
    });
    let entry = matched.ok_or_else(|| format!("Game '{}' is not available on this peer", game_key))?;
    let dir = if !entry.install_dir.trim().is_empty() {
        PathBuf::from(entry.install_dir.trim())
    } else {
        resolve_game_root_dir(Path::new(entry.exe_path.trim()))
    };
    if !dir.is_dir() {
        return Err(format!("Peer game directory missing: {}", dir.display()));
    }
    Ok(dir)
}

async fn write_frame<T: Serialize>(stream: &mut TcpStream, msg: &T) -> Result<(), String> {
    let payload = serde_json::to_vec(msg).map_err(|e| format!("Encode frame error: {}", e))?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err("Swarm frame exceeds 32 MiB ceiling".to_string());
    }
    let len_bytes = (payload.len() as u32).to_le_bytes();
    stream
        .write_all(&len_bytes)
        .await
        .map_err(|e| format!("TCP write len error: {}", e))?;
    stream
        .write_all(&payload)
        .await
        .map_err(|e| format!("TCP write body error: {}", e))?;
    stream
        .flush()
        .await
        .map_err(|e| format!("TCP flush error: {}", e))
}

async fn read_frame<T: for<'de> Deserialize<'de>>(stream: &mut TcpStream) -> Result<T, String> {
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .await
        .map_err(|e| format!("TCP read len error: {}", e))?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > MAX_FRAME_BYTES {
        return Err(format!("Invalid Swarm frame length: {}", len));
    }
    let mut buf = vec![0u8; len];
    stream
        .read_exact(&mut buf)
        .await
        .map_err(|e| format!("TCP read body error: {}", e))?;
    serde_json::from_slice::<T>(&buf).map_err(|e| format!("Decode frame error: {}", e))
}

fn collect_chunks_bundle_bytes(
    source_dir: &Path,
    manifest: &DkManifest,
    hashes: &HashSet<String>,
) -> Result<Vec<u8>, String> {
    let tmp = std::env::temp_dir().join(format!(
        "duke_swarm_delta_{}_{}.dkdelta",
        std::process::id(),
        now_secs()
    ));
    create_zstd_delta_bundle(source_dir, manifest, hashes, &tmp)?;
    let bytes = fs::read(&tmp)
        .map_err(|e| format!("Failed reading streamed delta bundle: {}", e))?;
    let _ = fs::remove_file(&tmp);
    Ok(bytes)
}

async fn build_symbolic_save_staging(game_key: &str) -> Result<Option<(PathBuf, GameEntry)>, String> {
    let library = load_library_standalone()?;
    let Some(entry) = library
        .into_iter()
        .find(|g| g.id == game_key || game_key_for_entry(g) == game_key)
    else {
        return Ok(None);
    };
    let report = crate::ludusavi::discover_game_save_paths_core(&entry.id, false).await?;
    let staging_dir = get_app_data_file_standalone("swarm_saves")?
        .join(sanitize_filename(&game_key_for_entry(&entry)));
    let _ = fs::remove_dir_all(&staging_dir);
    fs::create_dir_all(&staging_dir)
        .map_err(|e| format!("Failed creating save staging dir: {}", e))?;

    let appdata = std::env::var("APPDATA").unwrap_or_default();
    let localappdata = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let userprofile = std::env::var("USERPROFILE").unwrap_or_default();
    let game_dir = if !entry.install_dir.trim().is_empty() {
        entry.install_dir.trim().to_string()
    } else {
        resolve_game_root_dir(Path::new(entry.exe_path.trim()))
            .to_string_lossy()
            .into_owned()
    };

    let mut source_roots: Vec<PathBuf> = report
        .existing_paths
        .iter()
        .map(|p| PathBuf::from(&p.path))
        .collect();
    let sandbox_dir = PathBuf::from(&game_dir).join(".duke-profile");
    if sandbox_dir.is_dir() {
        source_roots.push(sandbox_dir);
    }

    let emus_root = crate::emulators::emulators_root_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    let roots: [(&str, String); 5] = [
        ("__DUKE_EMUS__", emus_root),
        ("__GAME__", game_dir.clone()),
        ("__APPDATA__", appdata.clone()),
        ("__LOCALAPPDATA__", localappdata.clone()),
        ("__USERPROFILE__", userprofile.clone()),
    ];

    for src_path in source_roots {
        if !src_path.exists() {
            continue;
        }
        let src_lower = src_path.to_string_lossy().to_ascii_lowercase();
        let mut chosen_bucket = "__ABSOLUTE__";
        let mut relative_sub = src_path
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("save"));

        for (bucket, root_str) in &roots {
            if root_str.is_empty() {
                continue;
            }
            let root_lower = root_str.to_ascii_lowercase();
            if src_lower.starts_with(&root_lower) {
                let src_full = src_path.to_string_lossy();
                let tail = src_full
                    .get(root_str.len()..)
                    .unwrap_or("")
                    .trim_start_matches(['\\', '/']);
                if !tail.is_empty() {
                    chosen_bucket = bucket;
                    relative_sub = PathBuf::from(tail.replace('\\', "/"));
                    break;
                }
            }
        }

        let dest_root = staging_dir.join(chosen_bucket).join(relative_sub);
        if src_path.is_file() {
            if let Some(p) = dest_root.parent() {
                let _ = fs::create_dir_all(p);
            }
            let _ = fs::copy(&src_path, &dest_root);
        } else if src_path.is_dir() {
            for item in walkdir::WalkDir::new(&src_path)
                .follow_links(false)
                .into_iter()
                .flatten()
            {
                if !item.file_type().is_file() {
                    continue;
                }
                if let Ok(rel) = item.path().strip_prefix(&src_path) {
                    let target = dest_root.join(rel);
                    if let Some(p) = target.parent() {
                        let _ = fs::create_dir_all(p);
                    }
                    let _ = fs::copy(item.path(), target);
                }
            }
        }
    }

    Ok(Some((staging_dir, entry)))
}

fn materialize_staged_saves_to_live_paths(staging_dir: &Path, entry: &GameEntry) -> Result<usize, String> {
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    let localappdata = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let userprofile = std::env::var("USERPROFILE").unwrap_or_default();
    let game_dir = if !entry.install_dir.trim().is_empty() {
        entry.install_dir.trim().to_string()
    } else {
        resolve_game_root_dir(Path::new(entry.exe_path.trim()))
            .to_string_lossy()
            .into_owned()
    };

    let emus_root = crate::emulators::emulators_root_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mappings: [(&str, &str); 5] = [
        ("__DUKE_EMUS__", &emus_root),
        ("__GAME__", &game_dir),
        ("__APPDATA__", &appdata),
        ("__LOCALAPPDATA__", &localappdata),
        ("__USERPROFILE__", &userprofile),
    ];

    let mut restored = 0usize;
    for (bucket, target_root) in mappings {
        if target_root.trim().is_empty() {
            continue;
        }
        let bucket_dir = staging_dir.join(bucket);
        if !bucket_dir.is_dir() {
            continue;
        }
        let root_path = Path::new(target_root);
        for item in walkdir::WalkDir::new(&bucket_dir)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            if !item.file_type().is_file() {
                continue;
            }
            if let Ok(rel) = item.path().strip_prefix(&bucket_dir) {
                let rel_norm = rel.to_string_lossy().replace('\\', "/");
                if let Ok(dest) = safe_join_rel_path(root_path, &rel_norm) {
                    if let Some(p) = dest.parent() {
                        let _ = fs::create_dir_all(p);
                    }
                    if fs::copy(item.path(), &dest).is_ok() {
                        restored += 1;
                    }
                }
            }
        }
    }
    Ok(restored)
}

async fn handle_tcp_connection(mut stream: TcpStream) -> Result<(), String> {
    let _ = stream.set_nodelay(true);
    let req: SwarmWireRequest = read_frame(&mut stream).await?;

    let response = match req {
        SwarmWireRequest::GetBeacon => {
            let port = *listening_port_slot().lock().unwrap();
            SwarmWireResponse::Beacon(build_local_beacon(port))
        }
        SwarmWireRequest::BeamManifest { game_key } => {
            let dir = resolve_game_dir_by_key(&game_key)?;
            let manifest = tokio::task::spawn_blocking(move || build_dkmanifest_from_dir(&dir, &game_key))
                .await
                .map_err(|e| format!("Manifest task failed: {}", e))??;
            SwarmWireResponse::Manifest(manifest)
        }
        SwarmWireRequest::BeamDeltaBundle {
            game_key,
            missing_hashes,
        } => {
            let dir = resolve_game_dir_by_key(&game_key)?;
            let bundle_bytes = tokio::task::spawn_blocking(move || {
                let manifest = build_dkmanifest_from_dir(&dir, &game_key)?;
                let wanted: HashSet<String> = missing_hashes.into_iter().collect();
                collect_chunks_bundle_bytes(&dir, &manifest, &wanted)
            })
            .await
            .map_err(|e| format!("Bundle task failed: {}", e))??;
            SwarmWireResponse::DeltaBundle { bundle_bytes }
        }
        SwarmWireRequest::SaveSyncOffer {
            save_key,
            sender_clock,
            manifest,
            delta_bytes,
        } => {
            let mut state = load_or_init_swarm_state();
            let local_clock = state.save_clocks.entry(save_key.clone()).or_default().clone();
            let ord = compare_vector_clocks(&local_clock, &sender_clock);
            let mut applied = false;
            let mut conflict_backup = false;

            if matches!(ord, ClockOrdering::Before | ClockOrdering::Concurrent) {
                if let Some((staging_dir, entry)) = build_symbolic_save_staging(&save_key).await? {
                    if matches!(ord, ClockOrdering::Concurrent) {
                        let _ = crate::ludusavi::backup_game_saves_core(
                            Some(entry.id.clone()),
                            Some(entry.title.clone()),
                            Some(entry.install_dir.clone()),
                            Some(entry.exe_path.clone()),
                        )
                        .await;
                        conflict_backup = true;
                    }
                    if let Some(target_manifest) = manifest {
                        let tmp_bundle = staging_dir.with_extension("incoming.dkdelta");
                        if let Some(bytes) = delta_bytes.as_ref() {
                            let _ = fs::write(&tmp_bundle, bytes);
                        }
                        let bundle_opt = tmp_bundle.is_file().then_some(tmp_bundle.as_path());
                        let _ = apply_delta_with_local_stitching(
                            &staging_dir,
                            &target_manifest,
                            bundle_opt,
                        )?;
                        let _ = materialize_staged_saves_to_live_paths(&staging_dir, &entry);
                        let _ = fs::remove_file(&tmp_bundle);
                        applied = true;
                    }
                    let slot = state.save_clocks.entry(save_key.clone()).or_default();
                    merge_vector_clocks(slot, &sender_clock);
                    save_swarm_state(&state)?;
                }
            }

            let updated_clock = state.save_clocks.get(&save_key).cloned().unwrap_or_default();
            SwarmWireResponse::SaveState {
                clock: updated_clock,
                manifest: None,
                bundle_bytes: None,
                applied,
                conflict_backup,
            }
        }
        SwarmWireRequest::SaveSyncPull {
            save_key,
            have_hashes,
        } => {
            let state = load_or_init_swarm_state();
            let clock = state.save_clocks.get(&save_key).cloned().unwrap_or_default();
            if let Some((staging_dir, _)) = build_symbolic_save_staging(&save_key).await? {
                let manifest = build_dkmanifest_from_dir(&staging_dir, &save_key)?;
                let have: HashSet<String> = have_hashes.into_iter().collect();
                let mut missing = HashSet::new();
                for f in &manifest.files {
                    for c in &f.chunks {
                        if !have.contains(&c.blake3) {
                            missing.insert(c.blake3.clone());
                        }
                    }
                }
                let bundle_bytes = collect_chunks_bundle_bytes(&staging_dir, &manifest, &missing)?;
                SwarmWireResponse::SaveState {
                    clock,
                    manifest: Some(manifest),
                    bundle_bytes: Some(bundle_bytes),
                    applied: false,
                    conflict_backup: false,
                }
            } else {
                SwarmWireResponse::SaveState {
                    clock,
                    manifest: None,
                    bundle_bytes: None,
                    applied: false,
                    conflict_backup: false,
                }
            }
        }
        SwarmWireRequest::ShaderPull {
            game_blake3,
            gpu_key,
            driver_version,
        } => crate::shader_swarm::serve_shader_pull(&game_blake3, &gpu_key, &driver_version),
    };

    write_frame(&mut stream, &response).await
}

fn record_discovered_peer(beacon: SwarmBeacon, remote_ip: IpAddr, transport: &str) {
    let (local_id, _) = local_node_identity();
    if beacon.magic != SWARM_MAGIC || beacon.node_id == local_id {
        return;
    }
    let addr = format!("{}:{}", remote_ip, beacon.tcp_port);
    let info = SwarmPeerInfo {
        node_id: beacon.node_id.clone(),
        node_name: beacon.node_name,
        addr,
        tcp_port: beacon.tcp_port,
        transport: transport.to_string(),
        last_seen_secs: now_secs(),
        apps: beacon.apps,
        save_clocks: beacon.save_clocks,
    };
    peer_registry().lock().unwrap().insert(beacon.node_id, info);
}

fn normalize_peer_socket_addr(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.contains(':') {
        trimmed.to_string()
    } else {
        format!("{}:{}", trimmed, SWARM_DEFAULT_TCP_PORT)
    }
}

pub(crate) async fn send_tcp_request_public(
    addr: &str,
    req: &SwarmWireRequest,
) -> Result<SwarmWireResponse, String> {
    send_tcp_request(addr, req).await
}

pub(crate) fn known_peer_addrs_public() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    if let Ok(guard) = peer_registry().lock() {
        for peer in guard.values() {
            if seen.insert(peer.addr.clone()) {
                out.push((peer.node_name.clone(), peer.addr.clone()));
            }
        }
    }
    let settings = crate::load_settings_standalone();
    for raw in settings.swarm_peers {
        let norm = normalize_peer_socket_addr(&raw);
        if seen.insert(norm.clone()) {
            out.push((raw, norm));
        }
    }
    out
}

async fn send_tcp_request(addr: &str, req: &SwarmWireRequest) -> Result<SwarmWireResponse, String> {
    let target = normalize_peer_socket_addr(addr);
    let mut stream = tokio::time::timeout(
        std::time::Duration::from_secs(4),
        TcpStream::connect(&target),
    )
    .await
    .map_err(|_| format!("Timed out connecting to Swarm peer {}", target))?
    .map_err(|e| format!("Failed connecting to Swarm peer {}: {}", target, e))?;
    let _ = stream.set_nodelay(true);
    write_frame(&mut stream, req).await?;
    read_frame(&mut stream).await
}

pub fn start_swarm_background_services() {
    SWARM_STARTED.get_or_init(|| {
        tauri::async_runtime::spawn(async move {
            let bind_candidates = [
                format!("0.0.0.0:{}", SWARM_DEFAULT_TCP_PORT),
                "0.0.0.0:0".to_string(),
            ];
            let mut listener = None;
            for addr in bind_candidates {
                if let Ok(l) = TcpListener::bind(&addr).await {
                    listener = Some(l);
                    break;
                }
            }
            if let Some(listener) = listener {
                let bound_port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
                *listening_port_slot().lock().unwrap() = bound_port;

                tauri::async_runtime::spawn(async move {
                    loop {
                        if let Ok((stream, _)) = listener.accept().await {
                            tauri::async_runtime::spawn(async move {
                                let _ = handle_tcp_connection(stream).await;
                            });
                        }
                    }
                });
            }

            // UDP Multicast beacon listener (239.255.77.88:47880).
            if let Ok(udp) = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], SWARM_DISCOVERY_PORT))).await {
                let _ = udp.join_multicast_v4(SWARM_MULTICAST_V4, Ipv4Addr::UNSPECIFIED);
                let recv_sock = std::sync::Arc::new(udp);
                let send_sock = std::sync::Arc::clone(&recv_sock);

                tauri::async_runtime::spawn(async move {
                    let mut buf = vec![0u8; 32 * 1024];
                    loop {
                        if let Ok((n, from_addr)) = recv_sock.recv_from(&mut buf).await {
                            if let Ok(beacon) = serde_json::from_slice::<SwarmBeacon>(&buf[..n]) {
                                record_discovered_peer(beacon, from_addr.ip(), "lan-multicast");
                            }
                        }
                    }
                });

                tauri::async_runtime::spawn(async move {
                    let mcast_target = SocketAddr::new(IpAddr::V4(SWARM_MULTICAST_V4), SWARM_DISCOVERY_PORT);
                    loop {
                        let settings = load_settings_standalone();
                        if settings.swarm_enabled {
                            let port = *listening_port_slot().lock().unwrap();
                            if port > 0 {
                                let beacon = build_local_beacon(port);
                                if let Ok(bytes) = serde_json::to_vec(&beacon) {
                                    let _ = send_sock.send_to(&bytes, mcast_target).await;
                                }
                            }
                            for peer in settings.swarm_peers {
                                let trimmed = peer.trim();
                                if trimmed.is_empty() {
                                    continue;
                                }
                                if let Ok(SwarmWireResponse::Beacon(b)) =
                                    send_tcp_request(trimmed, &SwarmWireRequest::GetBeacon).await
                                {
                                    let norm = normalize_peer_socket_addr(trimmed);
                                    if let Ok(sock) = norm.parse::<SocketAddr>() {
                                        record_discovered_peer(b, sock.ip(), "direct-tailscale");
                                    }
                                }
                            }
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(6)).await;
                    }
                });
            }
        });
    });
}

fn snapshot_swarm_status() -> SwarmStatusSnapshot {
    let settings = load_settings_standalone();
    let (node_id, node_name) = local_node_identity();
    let port = *listening_port_slot().lock().unwrap();
    let now = now_secs();
    let mut map = peer_registry().lock().unwrap();
    map.retain(|_, p| now.saturating_sub(p.last_seen_secs) <= 120);
    let mut peers: Vec<SwarmPeerInfo> = map.values().cloned().collect();
    peers.sort_by(|a, b| a.node_name.cmp(&b.node_name));

    SwarmStatusSnapshot {
        enabled: settings.swarm_enabled,
        node_id,
        node_name,
        tcp_port: port,
        multicast_group: format!("{}:{}", SWARM_MULTICAST_V4, SWARM_DISCOVERY_PORT),
        configured_peers: settings.swarm_peers,
        peers,
    }
}

pub fn snapshot_swarm_status_public() -> SwarmStatusSnapshot {
    snapshot_swarm_status()
}

pub async fn beam_game_over_tcp(
    peer_addr: &str,
    game_key: &str,
    dest_dir: &Path,
) -> Result<BeamTransferReport, String> {
    let started = Instant::now();
    let peer_beacon = match send_tcp_request(peer_addr, &SwarmWireRequest::GetBeacon).await? {
        SwarmWireResponse::Beacon(b) => b,
        SwarmWireResponse::Error { message } => return Err(message),
        _ => return Err("Unexpected response to GetBeacon".to_string()),
    };

    let manifest = match send_tcp_request(
        peer_addr,
        &SwarmWireRequest::BeamManifest {
            game_key: game_key.to_string(),
        },
    )
    .await?
    {
        SwarmWireResponse::Manifest(m) => m,
        SwarmWireResponse::Error { message } => return Err(message),
        _ => return Err("Unexpected response to BeamManifest".to_string()),
    };

    let dest_clone = dest_dir.to_path_buf();
    let manifest_clone = manifest.clone();
    let diff = tokio::task::spawn_blocking(move || {
        diff_local_dir_against_manifest(&dest_clone, &manifest_clone)
    })
    .await
    .map_err(|e| format!("Local delta diff failed: {}", e))??;

    let mut downloaded_delta_bytes = 0u64;
    let tmp_bundle = dest_dir.join(format!(".duke_beam_{}.dkdelta", std::process::id()));

    if !diff.missing_hashes.is_empty() {
        let bundle_bytes = match send_tcp_request(
            peer_addr,
            &SwarmWireRequest::BeamDeltaBundle {
                game_key: game_key.to_string(),
                missing_hashes: diff.missing_hashes.clone(),
            },
        )
        .await?
        {
            SwarmWireResponse::DeltaBundle { bundle_bytes } => bundle_bytes,
            SwarmWireResponse::Error { message } => return Err(message),
            _ => return Err("Unexpected response to BeamDeltaBundle".to_string()),
        };
        downloaded_delta_bytes = bundle_bytes.len() as u64;
        fs::create_dir_all(dest_dir)
            .map_err(|e| format!("Failed creating destination {}: {}", dest_dir.display(), e))?;
        fs::write(&tmp_bundle, &bundle_bytes)
            .map_err(|e| format!("Failed staging beamed bundle: {}", e))?;
    }

    let dest_apply = dest_dir.to_path_buf();
    let bundle_arg = tmp_bundle.is_file().then_some(tmp_bundle.clone());
    let manifest_apply = manifest.clone();
    let apply_res = tokio::task::spawn_blocking(move || {
        apply_delta_with_local_stitching(&dest_apply, &manifest_apply, bundle_arg.as_deref())
    })
    .await
    .map_err(|e| format!("Game-Beam apply task failed: {}", e))?;
    let _ = fs::remove_file(&tmp_bundle);
    apply_res?;

    let elapsed_ms = started.elapsed().as_millis().max(1) as u64;
    let throughput_mbps =
        (downloaded_delta_bytes as f64 * 8.0 / 1_000_000.0) / (elapsed_ms as f64 / 1000.0);

    let title = peer_beacon
        .apps
        .iter()
        .find(|a| a.game_key == game_key || a.local_id == game_key || a.title.eq_ignore_ascii_case(game_key))
        .map(|a| a.title.clone())
        .unwrap_or_else(|| game_key.to_string());

    Ok(BeamTransferReport {
        peer_name: peer_beacon.node_name,
        game_title: title,
        dest_dir: dest_dir.to_string_lossy().into_owned(),
        files_total: manifest.files.len(),
        total_bytes: manifest.total_bytes,
        downloaded_delta_bytes,
        reused_bytes: diff.reused_bytes,
        throughput_mbps: (throughput_mbps * 100.0).round() / 100.0,
        elapsed_ms,
        registered_game_id: None,
    })
}

pub async fn sync_game_saves_mesh_core(game_id: &str) -> Result<MeshSaveSyncReport, String> {
    let Some((staging_dir, entry)) = build_symbolic_save_staging(game_id).await? else {
        return Err(format!("Game '{}' not found for Mesh Save Sync.", game_id));
    };
    let save_key = game_key_for_entry(&entry);
    let manifest = build_dkmanifest_from_dir(&staging_dir, &entry.title)?;

    let mut digest_acc = blake3::Hasher::new();
    for f in &manifest.files {
        digest_acc.update(f.rel_path.as_bytes());
        digest_acc.update(f.blake3.as_bytes());
    }
    let current_digest = digest_acc.finalize().to_hex().to_string();

    let mut state = load_or_init_swarm_state();
    let prev_digest = state.save_digests.get(&save_key).cloned().unwrap_or_default();
    let local_clock = state.save_clocks.entry(save_key.clone()).or_default();
    if current_digest != prev_digest && !manifest.files.is_empty() {
        increment_vector_clock(local_clock, &state.node_id);
        state.save_digests.insert(save_key.clone(), current_digest);
        save_swarm_state(&state)?;
    }

    let local_clock_snap = state.save_clocks.get(&save_key).cloned().unwrap_or_default();
    let mut all_hashes = HashSet::new();
    for f in &manifest.files {
        for c in &f.chunks {
            all_hashes.insert(c.blake3.clone());
        }
    }
    let delta_bytes = if manifest.files.is_empty() {
        Vec::new()
    } else {
        collect_chunks_bundle_bytes(&staging_dir, &manifest, &all_hashes)?
    };

    let status = snapshot_swarm_status();
    let mut peer_targets: Vec<String> = status.peers.iter().map(|p| p.addr.clone()).collect();
    for cfg in &status.configured_peers {
        let norm = normalize_peer_socket_addr(cfg);
        if !peer_targets.contains(&norm) {
            peer_targets.push(norm);
        }
    }

    let mut peers_checked = 0usize;
    let mut peers_synced = 0usize;
    let mut bytes_pushed = 0u64;
    let mut bytes_pulled = 0u64;
    let mut conflict_backup_created = false;

    for peer in peer_targets {
        if peer.trim().is_empty() {
            continue;
        }
        peers_checked += 1;
        let req = SwarmWireRequest::SaveSyncOffer {
            save_key: save_key.clone(),
            sender_clock: local_clock_snap.clone(),
            manifest: Some(manifest.clone()),
            delta_bytes: Some(delta_bytes.clone()),
        };
        if let Ok(SwarmWireResponse::SaveState {
            applied,
            conflict_backup,
            ..
        }) = send_tcp_request(&peer, &req).await
        {
            if conflict_backup {
                conflict_backup_created = true;
            }
            if applied {
                peers_synced += 1;
                bytes_pushed += delta_bytes.len() as u64;
            }
            let pull_req = SwarmWireRequest::SaveSyncPull {
                save_key: save_key.clone(),
                have_hashes: all_hashes.iter().cloned().collect(),
            };
            if let Ok(SwarmWireResponse::SaveState {
                clock: remote_clock,
                manifest: Some(remote_manifest),
                bundle_bytes: Some(remote_bundle),
                ..
            }) = send_tcp_request(&peer, &pull_req).await
            {
                let ord = compare_vector_clocks(&local_clock_snap, &remote_clock);
                if matches!(ord, ClockOrdering::Before | ClockOrdering::Concurrent) {
                    if matches!(ord, ClockOrdering::Concurrent) {
                        let _ = crate::ludusavi::backup_game_saves_core(
                            Some(entry.id.clone()),
                            Some(entry.title.clone()),
                            Some(entry.install_dir.clone()),
                            Some(entry.exe_path.clone()),
                        )
                        .await;
                        conflict_backup_created = true;
                    }
                    let pull_tmp = staging_dir.with_extension("pull.dkdelta");
                    let _ = fs::write(&pull_tmp, &remote_bundle);
                    if apply_delta_with_local_stitching(
                        &staging_dir,
                        &remote_manifest,
                        Some(&pull_tmp),
                    )
                    .is_ok()
                    {
                        let _ = materialize_staged_saves_to_live_paths(&staging_dir, &entry);
                        bytes_pulled += remote_bundle.len() as u64;
                        peers_synced += 1;
                        let mut st = load_or_init_swarm_state();
                        let clk = st.save_clocks.entry(save_key.clone()).or_default();
                        merge_vector_clocks(clk, &remote_clock);
                        let _ = save_swarm_state(&st);
                    }
                    let _ = fs::remove_file(&pull_tmp);
                }
            }
        }
    }

    let final_clock = load_or_init_swarm_state()
        .save_clocks
        .get(&save_key)
        .cloned()
        .unwrap_or_default();

    Ok(MeshSaveSyncReport {
        game_id: entry.id,
        game_title: entry.title.clone(),
        save_key,
        local_clock: final_clock,
        peers_checked,
        peers_synced,
        bytes_pushed,
        bytes_pulled,
        conflict_backup_created,
        summary: format!(
            "Mesh synced saves for '{}' across {} peers ({} updated, pushed {}, pulled {}).",
            entry.title,
            peers_checked,
            peers_synced,
            format_bytes(bytes_pushed),
            format_bytes(bytes_pulled)
        ),
    })
}

pub fn spawn_game_stopped_mesh_sync(game_id: String) {
    let settings = load_settings_standalone();
    if !settings.swarm_enabled {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let _ = sync_game_saves_mesh_core(&game_id).await;
    });
}

#[tauri::command]
pub async fn get_swarm_status() -> Result<SwarmStatusSnapshot, String> {
    start_swarm_background_services();
    Ok(snapshot_swarm_status())
}

#[tauri::command]
pub async fn probe_swarm_peers() -> Result<SwarmStatusSnapshot, String> {
    start_swarm_background_services();
    let settings = load_settings_standalone();
    for raw_peer in &settings.swarm_peers {
        let trimmed = raw_peer.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(SwarmWireResponse::Beacon(b)) =
            send_tcp_request(trimmed, &SwarmWireRequest::GetBeacon).await
        {
            if let Ok(sock) = normalize_peer_socket_addr(trimmed).parse::<SocketAddr>() {
                record_discovered_peer(b, sock.ip(), "direct-tailscale");
            }
        }
    }
    Ok(snapshot_swarm_status())
}

#[tauri::command]
pub async fn beam_game_from_peer(
    peer_addr: String,
    game_key: String,
    dest_parent_dir: Option<String>,
    register_in_library: Option<bool>,
) -> Result<BeamTransferReport, String> {
    let parent = dest_parent_dir
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            crate::resolve_data_root_dir_standalone()
                .unwrap_or_else(|_| std::env::temp_dir())
                .join("SwarmBeams")
        });

    let folder_name = sanitize_filename(&game_key.replace(':', "_"));
    let dest_dir = parent.join(folder_name);
    let mut report = beam_game_over_tcp(peer_addr.trim(), game_key.trim(), &dest_dir).await?;

    if register_in_library.unwrap_or(true) {
        if let Some(best_exe) = find_best_game_exe_in_folder(&dest_dir, &report.game_title) {
            let mut library = load_library_standalone().unwrap_or_default();
            let exe_str = best_exe.to_string_lossy().into_owned();
            if !library.iter().any(|g| g.exe_path.eq_ignore_ascii_case(&exe_str)) {
                let id = format!("game-swarm-{}", now_secs());
                let size_bytes = calculate_dir_size_sync(&dest_dir);
                library.push(GameEntry {
                    id: id.clone(),
                    title: clean_game_title(&report.game_title),
                    exe_path: exe_str,
                    install_dir: dest_dir.to_string_lossy().into_owned(),
                    size_bytes,
                    size_formatted: format_bytes(size_bytes),
                    source: "Duke Swarm".to_string(),
                    acquisition_source: "swarm".to_string(),
                    platform: "PC".to_string(),
                    is_installed: true,
                    added_at: now_secs(),
                    ..GameEntry::default()
                });
                let _ = save_library_standalone(&library);
                crate::sidebar_icons::invalidate_sidebar_icon_standalone(&id);
                report.registered_game_id = Some(id);
            }
        }
    }

    Ok(report)
}

#[tauri::command]
pub async fn sync_game_saves_mesh(game_id: String) -> Result<MeshSaveSyncReport, String> {
    sync_game_saves_mesh_core(&game_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_clock_partial_order_detects_before_after_and_conflict() {
        let mut a = VectorClock::new();
        increment_vector_clock(&mut a, "node-1");

        let mut b = a.clone();
        increment_vector_clock(&mut b, "node-1");
        assert_eq!(compare_vector_clocks(&a, &b), ClockOrdering::Before);
        assert_eq!(compare_vector_clocks(&b, &a), ClockOrdering::After);
        assert_eq!(compare_vector_clocks(&a, &a), ClockOrdering::Equal);

        let mut c = a.clone();
        increment_vector_clock(&mut c, "node-2");
        assert_eq!(compare_vector_clocks(&b, &c), ClockOrdering::Concurrent);

        merge_vector_clocks(&mut b, &c);
        assert_eq!(b.get("node-1"), Some(&2));
        assert_eq!(b.get("node-2"), Some(&1));
        assert_eq!(compare_vector_clocks(&b, &c), ClockOrdering::After);
    }

    #[tokio::test]
    async fn tcp_fastcdc_game_beam_streams_and_verifies_files() {
        let root = std::env::temp_dir().join(format!("duke_beam_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let src_dir = root.join("src_game");
        let dst_dir = root.join("dst_game");
        fs::create_dir_all(src_dir.join("data")).unwrap();

        let payload = vec![0x7Du8; 180_000];
        let payload_len = payload.len() as u64;
        fs::write(src_dir.join("game.exe"), b"MZ-dummy-exe").unwrap();
        fs::write(src_dir.join("data/pack.bin"), &payload).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let src_clone = src_dir.clone();
        let server = tokio::spawn(async move {
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let req: SwarmWireRequest = read_frame(&mut stream).await.unwrap();
                let resp = match req {
                    SwarmWireRequest::GetBeacon => SwarmWireResponse::Beacon(SwarmBeacon {
                        magic: SWARM_MAGIC.to_string(),
                        node_id: "peer-src".to_string(),
                        node_name: "Desktop-A".to_string(),
                        tcp_port: addr.port(),
                        apps: vec![SwarmAppAd {
                            game_key: "test-game".to_string(),
                            local_id: "g1".to_string(),
                            title: "Test Game".to_string(),
                            app_id: None,
                            size_bytes: payload_len,
                        }],
                        save_clocks: BTreeMap::new(),
                        timestamp_secs: now_secs(),
                    }),
                    SwarmWireRequest::BeamManifest { .. } => {
                        let m = build_dkmanifest_from_dir(&src_clone, "test-game").unwrap();
                        SwarmWireResponse::Manifest(m)
                    }
                    SwarmWireRequest::BeamDeltaBundle { missing_hashes, .. } => {
                        let m = build_dkmanifest_from_dir(&src_clone, "test-game").unwrap();
                        let set: HashSet<String> = missing_hashes.into_iter().collect();
                        let bytes = collect_chunks_bundle_bytes(&src_clone, &m, &set).unwrap();
                        SwarmWireResponse::DeltaBundle {
                            bundle_bytes: bytes,
                        }
                    }
                    _ => SwarmWireResponse::Error {
                        message: "unexpected".to_string(),
                    },
                };
                write_frame(&mut stream, &resp).await.unwrap();
            }
        });

        let report =
            beam_game_over_tcp(&addr.to_string(), "test-game", &dst_dir)
                .await
                .unwrap();
        server.await.unwrap();

        assert_eq!(report.files_total, 2);
        assert_eq!(fs::read(dst_dir.join("game.exe")).unwrap(), b"MZ-dummy-exe");
        assert_eq!(fs::read(dst_dir.join("data/pack.bin")).unwrap(), payload);

        let _ = fs::remove_dir_all(&root);
    }
}
