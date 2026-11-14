mod protocol;
mod storage;

use protocol::{
    compare_vector_clocks, ClockOrdering, SwarmBeacon, SwarmWireRequest, SwarmWireResponse,
    MAX_FRAME_BYTES, SWARM_DEFAULT_TCP_PORT, SWARM_DISCOVERY_PORT, SWARM_MAGIC,
    SWARM_MULTICAST_V4,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::env;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use storage::SwarmStorage;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

#[derive(Debug, Clone, PartialEq, Eq)]
struct NodeCliConfig {
    bind: String,
    storage_dir: PathBuf,
    peers: Vec<String>,
    node_name: String,
    multicast_enabled: bool,
}

impl Default for NodeCliConfig {
    fn default() -> Self {
        Self {
            bind: format!("0.0.0.0:{}", SWARM_DEFAULT_TCP_PORT),
            storage_dir: default_storage_dir(),
            peers: Vec::new(),
            node_name: env::var("HOSTNAME")
                .or_else(|_| env::var("COMPUTERNAME"))
                .unwrap_or_else(|_| "duke-swarm-node".to_string()),
            multicast_enabled: true,
        }
    }
}

fn default_storage_dir() -> PathBuf {
    if let Ok(xdg) = env::var("XDG_DATA_HOME") {
        if !xdg.trim().is_empty() {
            return PathBuf::from(xdg).join("duke-swarm-node");
        }
    }
    if let Ok(local) = env::var("LOCALAPPDATA") {
        if !local.trim().is_empty() {
            return PathBuf::from(local).join("duke-swarm-node");
        }
    }
    if let Ok(home) = env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("duke-swarm-node");
        }
    }
    PathBuf::from("./duke-swarm-data")
}

fn parse_cli_args<I>(args: I) -> Result<Option<NodeCliConfig>, String>
where
    I: IntoIterator<Item = String>,
{
    let mut cfg = NodeCliConfig::default();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return Ok(None);
            }
            "--bind" => {
                let val = it
                    .next()
                    .ok_or_else(|| "--bind requires an address such as 0.0.0.0:47881".to_string())?;
                cfg.bind = val;
            }
            "--storage-dir" => {
                let val = it
                    .next()
                    .ok_or_else(|| "--storage-dir requires a directory path".to_string())?;
                cfg.storage_dir = PathBuf::from(val);
            }
            "--peers" => {
                let val = it
                    .next()
                    .ok_or_else(|| "--peers requires a comma-separated peer list".to_string())?;
                for item in val.split(',') {
                    let trimmed = item.trim();
                    if !trimmed.is_empty() {
                        cfg.peers.push(normalize_peer_addr(trimmed));
                    }
                }
            }
            "--node-name" => {
                let val = it
                    .next()
                    .ok_or_else(|| "--node-name requires a name".to_string())?;
                if !val.trim().is_empty() {
                    cfg.node_name = val.trim().to_string();
                }
            }
            "--no-multicast" => {
                cfg.multicast_enabled = false;
            }
            other => {
                return Err(format!("Unknown CLI flag '{}'. Pass --help for usage.", other));
            }
        }
    }
    Ok(Some(cfg))
}

fn print_help() {
    println!(
        "Duke Swarm Headless Node (v{})\n\n\
         Usage:\n  duke-swarm-node [--bind 0.0.0.0:47881] [--storage-dir <path>] [--peers ip1,ip2:47881] [--node-name <name>] [--no-multicast]\n",
        env!("CARGO_PKG_VERSION")
    );
}

fn normalize_peer_addr(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.contains(':') {
        trimmed.to_string()
    } else {
        format!("{}:{}", trimmed, SWARM_DEFAULT_TCP_PORT)
    }
}

async fn write_frame<T: Serialize>(stream: &mut TcpStream, msg: &T) -> Result<(), String> {
    let payload = serde_json::to_vec(msg).map_err(|e| format!("Encode frame error: {}", e))?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err("Swarm frame exceeds 32 MiB ceiling".to_string());
    }
    stream
        .write_all(&(payload.len() as u32).to_le_bytes())
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

async fn send_tcp_request(addr: &str, req: &SwarmWireRequest) -> Result<SwarmWireResponse, String> {
    let mut stream = tokio::time::timeout(Duration::from_secs(6), TcpStream::connect(addr))
        .await
        .map_err(|_| format!("Timed out connecting to {}", addr))?
        .map_err(|e| format!("Failed connecting to {}: {}", addr, e))?;
    let _ = stream.set_nodelay(true);
    write_frame(&mut stream, req).await?;
    tokio::time::timeout(Duration::from_secs(30), read_frame(&mut stream))
        .await
        .map_err(|_| format!("Timed out reading response from {}", addr))?
}

async fn run_multicast_beacon_loop(
    storage: Arc<SwarmStorage>,
    discovered_peers: Arc<Mutex<HashMap<String, String>>>,
) {
    let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), SWARM_DISCOVERY_PORT);
    let Ok(recv_sock) = UdpSocket::bind(bind_addr).await else {
        return;
    };
    let _ = recv_sock.join_multicast_v4(SWARM_MULTICAST_V4, Ipv4Addr::UNSPECIFIED);

    let send_sock = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(s) => s,
        Err(_) => return,
    };
    let _ = send_sock.set_broadcast(true);
    let multicast_dest = SocketAddr::new(IpAddr::V4(SWARM_MULTICAST_V4), SWARM_DISCOVERY_PORT);

    let rx_storage = storage.clone();
    tokio::spawn(async move {
        let mut buf = [0u8; 65_535];
        let (local_id, _) = rx_storage.node_identity();
        while let Ok((len, src)) = recv_sock.recv_from(&mut buf).await {
            if let Ok(beacon) = serde_json::from_slice::<SwarmBeacon>(&buf[..len]) {
                if beacon.magic == SWARM_MAGIC && beacon.node_id != local_id {
                    let peer_addr = format!("{}:{}", src.ip(), beacon.tcp_port);
                    if let Ok(mut guard) = discovered_peers.lock() {
                        guard.insert(beacon.node_id, peer_addr);
                    }
                }
            }
        }
    });

    let mut ticker = tokio::time::interval(Duration::from_secs(5));
    loop {
        ticker.tick().await;
        let beacon = storage.build_beacon();
        if let Ok(bytes) = serde_json::to_vec(&beacon) {
            if bytes.len() <= 60_000 {
                let _ = send_sock.send_to(&bytes, multicast_dest).await;
            }
        }
    }
}

async fn run_peer_sync_loop(
    storage: Arc<SwarmStorage>,
    static_peers: Vec<String>,
    discovered_peers: Arc<Mutex<HashMap<String, String>>>,
) {
    let mut ticker = tokio::time::interval(Duration::from_secs(45));
    let (local_id, _) = storage.node_identity();
    loop {
        ticker.tick().await;
        let mut targets: HashSet<String> = static_peers.iter().cloned().collect();
        if let Ok(guard) = discovered_peers.lock() {
            for addr in guard.values() {
                targets.insert(addr.clone());
            }
        }
        for addr in targets {
            let Ok(SwarmWireResponse::Beacon(remote_beacon)) =
                send_tcp_request(&addr, &SwarmWireRequest::GetBeacon).await
            else {
                continue;
            };
            if remote_beacon.node_id == local_id {
                continue;
            }
            let local_clocks = storage.snapshot_save_clocks();
            for (save_key, remote_clock) in &remote_beacon.save_clocks {
                let local_clock = local_clocks.get(save_key).cloned().unwrap_or_default();
                let ord = compare_vector_clocks(&local_clock, remote_clock);
                if matches!(ord, ClockOrdering::Before | ClockOrdering::Concurrent) {
                    let pull_req = storage.build_save_pull_request(save_key);
                    if let Ok(SwarmWireResponse::SaveState {
                        clock,
                        manifest,
                        bundle_bytes,
                        ..
                    }) = send_tcp_request(&addr, &pull_req).await
                    {
                        let _ = storage.apply_pulled_save(save_key, clock, manifest, bundle_bytes);
                    }
                }
            }
            for (save_key, local_clock) in &local_clocks {
                let remote_clock = remote_beacon
                    .save_clocks
                    .get(save_key)
                    .cloned()
                    .unwrap_or_default();
                if matches!(
                    compare_vector_clocks(local_clock, &remote_clock),
                    ClockOrdering::After
                ) {
                    if let Ok(Some(offer)) = storage.build_save_offer_request(save_key) {
                        let _ = send_tcp_request(&addr, &offer).await;
                    }
                }
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let cfg = match parse_cli_args(args) {
        Ok(Some(c)) => c,
        Ok(None) => return,
        Err(e) => {
            eprintln!("duke-swarm-node error: {}", e);
            std::process::exit(2);
        }
    };

    let bound: SocketAddr = match cfg.bind.parse() {
        Ok(addr) => addr,
        Err(e) => {
            eprintln!("Invalid --bind socket address '{}': {}", cfg.bind, e);
            std::process::exit(2);
        }
    };

    let storage = match SwarmStorage::open(&cfg.storage_dir, &cfg.node_name, bound.port()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed opening storage '{}': {}", cfg.storage_dir.display(), e);
            std::process::exit(1);
        }
    };

    let listener = match TcpListener::bind(bound).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed binding TCP listener on {}: {}", bound, e);
            std::process::exit(1);
        }
    };

    let discovered_peers = Arc::new(Mutex::new(HashMap::new()));
    let (node_id, node_name) = storage.node_identity();
    println!(
        "duke-swarm-node v{} online | node={} ({}) | tcp={} | storage={} | peers={}",
        env!("CARGO_PKG_VERSION"),
        node_name,
        node_id,
        bound,
        cfg.storage_dir.display(),
        cfg.peers.len()
    );

    if cfg.multicast_enabled {
        tokio::spawn(run_multicast_beacon_loop(
            storage.clone(),
            discovered_peers.clone(),
        ));
    }

    tokio::spawn(run_peer_sync_loop(
        storage.clone(),
        cfg.peers.clone(),
        discovered_peers,
    ));

    loop {
        tokio::select! {
            accept_res = listener.accept() => {
                let Ok((mut stream, _)) = accept_res else {
                    continue;
                };
                let st = storage.clone();
                tokio::spawn(async move {
                    let _ = stream.set_nodelay(true);
                    if let Ok(req) = read_frame::<SwarmWireRequest>(&mut stream).await {
                        let st_inner = st.clone();
                        let resp = tokio::task::spawn_blocking(move || st_inner.handle_request(req))
                            .await
                            .unwrap_or_else(|e| SwarmWireResponse::Error {
                                message: format!("Worker failed: {}", e),
                            });
                        let _ = write_frame(&mut stream, &resp).await;
                    }
                });
            }
            _ = tokio::signal::ctrl_c() => {
                println!("duke-swarm-node shutting down gracefully.");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{ChunkEntry, ManifestFileEntry, VectorClock};
    use storage::{encode_dkdelta_bytes, save_manifest_key};

    #[test]
    fn cli_flags_parse_correctly() {
        let cfg = parse_cli_args(vec![
            "--bind".into(),
            "100.64.0.5:47881".into(),
            "--storage-dir".into(),
            "/tmp/swarm-test".into(),
            "--peers".into(),
            "100.64.0.10,100.64.0.11:47999".into(),
            "--no-multicast".into(),
        ])
        .unwrap()
        .unwrap();

        assert_eq!(cfg.bind, "100.64.0.5:47881");
        assert_eq!(cfg.storage_dir, PathBuf::from("/tmp/swarm-test"));
        assert_eq!(
            cfg.peers,
            vec![
                "100.64.0.10:47881".to_string(),
                "100.64.0.11:47999".to_string()
            ]
        );
        assert!(!cfg.multicast_enabled);
    }

    #[test]
    fn headless_node_stores_manifests_blake3_saves_and_shaders() {
        let root = env::temp_dir().join(format!(
            "duke-swarm-node-test-{}-{}",
            std::process::id(),
            SystemTime::UNIX_EPOCH.elapsed().unwrap().as_nanos()
        ));
        let storage = SwarmStorage::open(&root, "pi5-relay", 47881).unwrap();

        let save_bytes = b"DUKE_SAVE_SLOT_1_HERO_LVL_99".to_vec();
        let save_hash = blake3::hash(&save_bytes).to_hex().to_string();
        let manifest = protocol::DkManifest {
            magic: "DKMF1".to_string(),
            root_label: "Hades II".to_string(),
            created_at: 1_760_000_000,
            chunk_min: 16 * 1024,
            chunk_avg: 32 * 1024,
            chunk_max: 64 * 1024,
            total_bytes: save_bytes.len() as u64,
            files: vec![ManifestFileEntry {
                rel_path: "__APPDATA__/Hades II/Profile1.sav".to_string(),
                size_bytes: save_bytes.len() as u64,
                blake3: save_hash.clone(),
                chunks: vec![ChunkEntry {
                    gear_hash: 0,
                    offset: 0,
                    length: save_bytes.len() as u32,
                    blake3: save_hash.clone(),
                }],
            }],
        };

        let mut chunks_map = HashMap::new();
        chunks_map.insert(save_hash.clone(), save_bytes.clone());
        let delta = encode_dkdelta_bytes(&chunks_map).unwrap();

        let mut clock = VectorClock::new();
        clock.insert("desktop-a".to_string(), 1);

        let resp = storage.handle_request(SwarmWireRequest::SaveSyncOffer {
            save_key: "game_hades2".to_string(),
            sender_clock: clock.clone(),
            manifest: Some(manifest.clone()),
            delta_bytes: Some(delta),
        });
        assert!(matches!(
            resp,
            SwarmWireResponse::SaveState { applied: true, .. }
        ));

        // Second offer from another PC increments a different node -> Concurrent conflict backup created
        let mut clock_b = VectorClock::new();
        clock_b.insert("laptop-b".to_string(), 1);
        let resp_conflict = storage.handle_request(SwarmWireRequest::SaveSyncOffer {
            save_key: "game_hades2".to_string(),
            sender_clock: clock_b,
            manifest: Some(manifest.clone()),
            delta_bytes: None,
        });
        assert!(matches!(
            resp_conflict,
            SwarmWireResponse::SaveState {
                applied: true,
                conflict_backup: true,
                ..
            }
        ));

        // Pull back from headless node with empty local hashes
        let pull_resp = storage.handle_request(SwarmWireRequest::SaveSyncPull {
            save_key: "game_hades2".to_string(),
            have_hashes: Vec::new(),
        });
        match pull_resp {
            SwarmWireResponse::SaveState {
                manifest: Some(m),
                bundle_bytes: Some(b),
                ..
            } => {
                assert_eq!(m.total_bytes, save_bytes.len() as u64);
                assert!(!b.is_empty());
            }
            other => panic!("Unexpected SaveSyncPull response: {:?}", other),
        }

        // Manifest storage & pull
        let mf_resp = storage.handle_request(SwarmWireRequest::ManifestOffer {
            manifest_key: "hades2_build".to_string(),
            manifest: manifest.clone(),
        });
        assert!(matches!(
            mf_resp,
            SwarmWireResponse::ManifestStored { stored: true, .. }
        ));
        let loaded_mf = storage.handle_request(SwarmWireRequest::BeamManifest {
            game_key: save_manifest_key("game_hades2"),
        });
        assert!(matches!(loaded_mf, SwarmWireResponse::Manifest(_)));

        // Shader bundle storage & pull
        let shd_offer = storage.handle_request(SwarmWireRequest::ShaderOffer {
            game_blake3: "deadbeef".to_string(),
            gpu_key: "10de:2684".to_string(),
            driver_version: "566.14".to_string(),
            file_count: 3,
            bundle_zst_bytes: b"DKSHD001_MOCK".to_vec(),
        });
        assert!(matches!(
            shd_offer,
            SwarmWireResponse::ShaderStored { stored: true, .. }
        ));
        let shd_pull = storage.handle_request(SwarmWireRequest::ShaderPull {
            game_blake3: "deadbeef".to_string(),
            gpu_key: "10de:2684".to_string(),
            driver_version: "566.14".to_string(),
        });
        assert!(matches!(
            shd_pull,
            SwarmWireResponse::ShaderBundle {
                found: true,
                file_count: 3,
                ..
            }
        ));

        let _ = fs::remove_dir_all(&root);
    }

    use std::fs;
    use std::time::SystemTime;
}
