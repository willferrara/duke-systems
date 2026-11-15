//! Duke Companion (`duke-companion`) — Headless SteamOS / Bazzite / Linux Tailscale Peer Daemon
//!
//! Synchronizes `.duke-profile/` sandboxes and game files with Windows `duke-launcher` over
//! Tailscale WireGuard (`100.64.0.0/10`, default TCP port `47881`) using length-prefixed JSON
//! frames, heap-allocated 64 KiB streaming BLAKE3 chunk verification, and deterministic
//! Proton `compatdata/<app_id>/pfx/` save-path mapping.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

pub const DEFAULT_COMPANION_PORT: u16 = 47881;
const CHUNK_BUF_BYTES: usize = 64 * 1024; // Heap-allocated 64 KiB streaming chunk buffer
const MAX_FRAME_BYTES: usize = 32 * 1024 * 1024;
const SWARM_MAGIC: &str = "DKSWRM1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionBeacon {
    pub magic: String,
    pub node_id: String,
    pub node_name: String,
    pub platform: String,
    pub tailscale_ip: Option<String>,
    pub tcp_port: u16,
    pub steam_apps: Vec<u32>,
    pub timestamp_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtonPathMapping {
    pub app_id: u32,
    pub duke_profile_path: PathBuf,
    pub proton_prefix_path: PathBuf,
    pub sync_direction: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedChunkEntry {
    pub rel_path: String,
    pub size_bytes: u64,
    pub blake3_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum CompanionWireRequest {
    GetBeacon,
    VerifyTree { rel_subdir: String },
    PullChunkedSave { rel_path: String, steam_app_id: Option<u32> },
    PushZstdSave {
        rel_path: String,
        steam_app_id: Option<u32>,
        expected_blake3: String,
        zstd_bytes: Vec<u8>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum CompanionWireResponse {
    Beacon(CompanionBeacon),
    TreeManifest { files: Vec<VerifiedChunkEntry> },
    ChunkedSave {
        rel_path: String,
        blake3_hex: String,
        zstd_bytes: Vec<u8>,
    },
    SaveApplied {
        rel_path: String,
        dest_path: String,
        blake3_hex: String,
        bytes_written: u64,
    },
    Error { message: String },
}

/// Validates whether an IPv4 address falls within Tailscale's RFC 6598 CGNAT subnet (`100.64.0.0/10`).
pub fn is_tailscale_cgnat_v4(ip: Ipv4Addr) -> bool {
    let oct = ip.octets();
    oct[0] == 100 && (64..=127).contains(&oct[1])
}

/// Discovers an active Tailscale CGNAT IPv4 address if WireGuard / Tailscale interface is up.
pub fn detect_tailscale_ipv4() -> Option<String> {
    if let Ok(explicit) = env::var("DUKE_TAILSCALE_IP") {
        let trimmed = explicit.trim();
        if let Ok(ip) = trimmed.parse::<Ipv4Addr>() {
            if is_tailscale_cgnat_v4(ip) {
                return Some(ip.to_string());
            }
        }
    }
    // UDP routing table probe toward Tailscale CGNAT space without transmitting packets
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        if sock.connect("100.64.0.1:41641").is_ok() {
            if let Ok(SocketAddr::V4(addr)) = sock.local_addr() {
                if is_tailscale_cgnat_v4(*addr.ip()) {
                    return Some(addr.ip().to_string());
                }
            }
        }
    }
    None
}

/// Safely joins a relative archive/sync path without allowing `..` traversal or drive roots.
pub fn safe_join_rel_path(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let clean = rel.replace('\\', "/");
    let mut joined = root.to_path_buf();
    for comp in Path::new(&clean).components() {
        match comp {
            Component::Normal(seg) => joined.push(seg),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("Blocked unsafe path traversal component in '{}'", rel));
            }
        }
    }
    Ok(joined)
}

/// Maps a relative Windows `.duke-profile/` save path into Proton's Wine prefix
/// under `<compatdata_root>/<steam_app_id>/pfx/drive_c/users/steamuser/...`, and vice-versa.
pub fn map_duke_profile_rel_to_proton(
    compatdata_root: &Path,
    steam_app_id: u32,
    duke_profile_rel: &str,
) -> Result<PathBuf, String> {
    let normalized = duke_profile_rel
        .trim()
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_string();
    let stripped = normalized
        .strip_prefix(".duke-profile/")
        .unwrap_or(&normalized);

    let lower = stripped.to_ascii_lowercase();
    let wine_rel = if lower.starts_with("appdata/roaming/") {
        format!("AppData/Roaming/{}", &stripped["appdata/roaming/".len()..])
    } else if lower.starts_with("appdata/local/") {
        format!("AppData/Local/{}", &stripped["appdata/local/".len()..])
    } else if lower.starts_with("appdata/locallow/") {
        format!("AppData/LocalLow/{}", &stripped["appdata/locallow/".len()..])
    } else if lower.starts_with("documents/") {
        format!("Documents/{}", &stripped["documents/".len()..])
    } else if lower.starts_with("saved_games/") {
        format!("Saved Games/{}", &stripped["saved_games/".len()..])
    } else if lower.starts_with("userprofile/") {
        stripped["userprofile/".len()..].to_string()
    } else {
        format!("Documents/DukeProfile/{}", stripped)
    };

    let pfx_user_root = compatdata_root
        .join(steam_app_id.to_string())
        .join("pfx")
        .join("drive_c")
        .join("users")
        .join("steamuser");
    safe_join_rel_path(&pfx_user_root, &wine_rel)
}

/// Reverse maps a Wine `drive_c/users/steamuser/...` path back into portable `.duke-profile/...` layout.
pub fn map_proton_user_rel_to_duke_profile(steamuser_rel: &str) -> Result<String, String> {
    let clean = steamuser_rel.trim().replace('\\', "/");
    let clean = clean.trim_start_matches('/');
    for seg in clean.split('/') {
        if seg == ".." || seg.contains(':') {
            return Err(format!("Blocked invalid path segment in '{}'", steamuser_rel));
        }
    }
    let lower = clean.to_ascii_lowercase();
    if lower.starts_with("appdata/roaming/") {
        Ok(format!(".duke-profile/appdata/Roaming/{}", &clean["appdata/roaming/".len()..]))
    } else if lower.starts_with("appdata/local/") {
        Ok(format!(".duke-profile/appdata/Local/{}", &clean["appdata/local/".len()..]))
    } else if lower.starts_with("appdata/locallow/") {
        Ok(format!(".duke-profile/appdata/LocalLow/{}", &clean["appdata/locallow/".len()..]))
    } else if lower.starts_with("saved games/") {
        Ok(format!(".duke-profile/saved_games/{}", &clean["saved games/".len()..]))
    } else if lower.starts_with("documents/") {
        Ok(format!(".duke-profile/documents/{}", &clean["documents/".len()..]))
    } else {
        Ok(format!(".duke-profile/userprofile/{}", clean))
    }
}

/// Streams a file through a heap-allocated 64 KiB buffer to compute `(blake3_hex, size_bytes)`.
pub fn hash_file_blake3_streaming(path: &Path) -> Result<(String, u64), String> {
    let mut file = File::open(path)
        .map_err(|e| format!("Failed opening '{}' for BLAKE3 hash: {}", path.display(), e))?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; CHUNK_BUF_BYTES];
    let mut total = 0u64;
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("Read error hashing '{}': {}", path.display(), e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total = total.saturating_add(n as u64);
    }
    Ok((hasher.finalize().to_hex().to_string(), total))
}

/// Reads and verifies raw or Zstd save bytes against an expected BLAKE3 hex digest before disk write.
pub fn decompress_and_verify_blake3(zstd_bytes: &[u8], expected_blake3: &str) -> Result<Vec<u8>, String> {
    let decoded = zstd::decode_all(zstd_bytes)
        .map_err(|e| format!("Failed Zstd decompression: {e}"))?;
    let actual = blake3::hash(&decoded).to_hex().to_string();
    if !expected_blake3.is_empty() && !actual.eq_ignore_ascii_case(expected_blake3.trim()) {
        return Err(format!(
            "BLAKE3 chunk verification failed (expected {}, got {}).",
            expected_blake3, actual
        ));
    }
    Ok(decoded)
}

pub fn build_verified_tree_manifest(root: &Path) -> Result<Vec<VerifiedChunkEntry>, String> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in WalkDir::new(root).follow_links(false).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(root) else {
            continue;
        };
        let (blake3_hex, size_bytes) = hash_file_blake3_streaming(entry.path())?;
        out.push(VerifiedChunkEntry {
            rel_path: rel.to_string_lossy().replace('\\', "/"),
            size_bytes,
            blake3_hex,
        });
    }
    out.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok(out)
}

pub fn write_wire_frame<T: Serialize>(stream: &mut TcpStream, msg: &T) -> Result<(), String> {
    let payload = serde_json::to_vec(msg).map_err(|e| format!("Frame encode error: {e}"))?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err("Companion frame exceeds 32 MiB safety cap".to_string());
    }
    let len_bytes = (payload.len() as u32).to_le_bytes();
    stream
        .write_all(&len_bytes)
        .and_then(|_| stream.write_all(&payload))
        .and_then(|_| stream.flush())
        .map_err(|e| format!("TCP frame write failed: {e}"))
}

pub fn read_wire_frame<T: for<'de> Deserialize<'de>>(stream: &mut TcpStream) -> Result<T, String> {
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .map_err(|e| format!("TCP frame header read error: {e}"))?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > MAX_FRAME_BYTES {
        return Err(format!("Invalid frame length: {len}"));
    }
    let mut buf = vec![0u8; len];
    stream
        .read_exact(&mut buf)
        .map_err(|e| format!("TCP frame body read error: {e}"))?;
    serde_json::from_slice::<T>(&buf).map_err(|e| format!("Frame decode error: {e}"))
}

pub fn discover_default_compatdata_root() -> PathBuf {
    if let Ok(explicit) = env::var("DUKE_PROTON_COMPATDATA") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    if let Ok(home) = env::var("HOME").or_else(|_| env::var("USERPROFILE")) {
        let linux_steam = PathBuf::from(&home).join(".local/share/Steam/steamapps/compatdata");
        if linux_steam.is_dir() {
            return linux_steam;
        }
        let deck_var = PathBuf::from(&home)
            .join(".var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/compatdata");
        if deck_var.is_dir() {
            return deck_var;
        }
    }
    env::temp_dir().join("duke-companion-compatdata")
}

pub fn handle_companion_request(
    req: CompanionWireRequest,
    data_root: &Path,
    compatdata_root: &Path,
    bind_port: u16,
) -> CompanionWireResponse {
    match req {
        CompanionWireRequest::GetBeacon => {
            let mut steam_apps = Vec::new();
            if let Ok(rd) = fs::read_dir(compatdata_root) {
                for ent in rd.flatten() {
                    if let Ok(app_id) = ent.file_name().to_string_lossy().parse::<u32>() {
                        steam_apps.push(app_id);
                    }
                }
            }
            steam_apps.sort_unstable();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let node_name = env::var("HOSTNAME")
                .or_else(|_| env::var("COMPUTERNAME"))
                .unwrap_or_else(|_| "duke-steamos-peer".to_string());
            CompanionWireResponse::Beacon(CompanionBeacon {
                magic: SWARM_MAGIC.to_string(),
                node_id: blake3::hash(node_name.as_bytes()).to_hex().to_string()[..16].to_string(),
                node_name,
                platform: std::env::consts::OS.to_string(),
                tailscale_ip: detect_tailscale_ipv4(),
                tcp_port: bind_port,
                steam_apps,
                timestamp_secs: now,
            })
        }
        CompanionWireRequest::VerifyTree { rel_subdir } => {
            match safe_join_rel_path(data_root, &rel_subdir)
                .and_then(|dir| build_verified_tree_manifest(&dir))
            {
                Ok(files) => CompanionWireResponse::TreeManifest { files },
                Err(message) => CompanionWireResponse::Error { message },
            }
        }
        CompanionWireRequest::PullChunkedSave {
            rel_path,
            steam_app_id,
        } => {
            let target = if let Some(app_id) = steam_app_id {
                map_duke_profile_rel_to_proton(compatdata_root, app_id, &rel_path)
            } else {
                safe_join_rel_path(data_root, &rel_path)
            };
            match target.and_then(|path| {
                let (blake3_hex, _) = hash_file_blake3_streaming(&path)?;
                let mut f = File::open(&path)
                    .map_err(|e| format!("Failed reading '{}': {}", path.display(), e))?;
                let mut raw = Vec::new();
                f.read_to_end(&mut raw)
                    .map_err(|e| format!("Failed reading save '{}': {}", path.display(), e))?;
                let zstd_bytes = zstd::encode_all(raw.as_slice(), 3)
                    .map_err(|e| format!("Zstd compression failed: {e}"))?;
                Ok((blake3_hex, zstd_bytes))
            }) {
                Ok((blake3_hex, zstd_bytes)) => CompanionWireResponse::ChunkedSave {
                    rel_path,
                    blake3_hex,
                    zstd_bytes,
                },
                Err(message) => CompanionWireResponse::Error { message },
            }
        }
        CompanionWireRequest::PushZstdSave {
            rel_path,
            steam_app_id,
            expected_blake3,
            zstd_bytes,
        } => {
            let target = if let Some(app_id) = steam_app_id {
                map_duke_profile_rel_to_proton(compatdata_root, app_id, &rel_path)
            } else {
                safe_join_rel_path(data_root, &rel_path)
            };
            match target.and_then(|dest| {
                let raw = decompress_and_verify_blake3(&zstd_bytes, &expected_blake3)?;
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|e| format!("Failed creating parent '{}': {}", parent.display(), e))?;
                }
                let tmp = dest.with_extension("duke.tmp");
                {
                    let mut f = File::create(&tmp)
                        .map_err(|e| format!("Failed staging save '{}': {}", tmp.display(), e))?;
                    f.write_all(&raw)
                        .map_err(|e| format!("Failed writing save '{}': {}", tmp.display(), e))?;
                }
                fs::rename(&tmp, &dest)
                    .map_err(|e| format!("Atomic rename failed '{}': {}", dest.display(), e))?;
                let actual_hex = blake3::hash(&raw).to_hex().to_string();
                Ok((dest, actual_hex, raw.len() as u64))
            }) {
                Ok((dest, blake3_hex, bytes_written)) => CompanionWireResponse::SaveApplied {
                    rel_path,
                    dest_path: dest.to_string_lossy().into_owned(),
                    blake3_hex,
                    bytes_written,
                },
                Err(message) => CompanionWireResponse::Error { message },
            }
        }
    }
}

pub fn serve_companion_daemon(
    bind_addr: &str,
    data_root: &Path,
    compatdata_root: &Path,
) -> Result<(), String> {
    let listener = TcpListener::bind(bind_addr)
        .map_err(|e| format!("Failed binding companion TCP daemon on {bind_addr}: {e}"))?;
    let port = listener
        .local_addr()
        .map(|a| a.port())
        .unwrap_or(DEFAULT_COMPANION_PORT);
    println!(
        "duke-companion listening on {bind_addr} (tailscale_v4={:?}, compatdata={})",
        detect_tailscale_ipv4(),
        compatdata_root.display()
    );
    for stream_res in listener.incoming() {
        let Ok(mut stream) = stream_res else {
            continue;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
        let data_c = data_root.to_path_buf();
        let compat_c = compatdata_root.to_path_buf();
        std::thread::spawn(move || {
            if let Ok(req) = read_wire_frame::<CompanionWireRequest>(&mut stream) {
                let resp = handle_companion_request(req, &data_c, &compat_c, port);
                let _ = write_wire_frame(&mut stream, &resp);
            }
        });
    }
    Ok(())
}

fn print_usage() {
    println!("duke-companion {} — SteamOS / Bazzite / Linux Tailscale Companion", env!("CARGO_PKG_VERSION"));
    println!("Usage:");
    println!("  duke-companion --serve [0.0.0.0:47881]");
    println!("  duke-companion --map-proton <app_id> <rel_duke_profile_path>");
    println!("  duke-companion --hash <file_path>");
    println!("  duke-companion --status");
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let compatdata = discover_default_compatdata_root();
    let data_root = env::var("DUKE_COMPANION_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| env::current_dir().unwrap_or_else(|_| env::temp_dir()));

    match args.first().map(String::as_str) {
        Some("--serve") => {
            let bind = args
                .get(1)
                .map(String::as_str)
                .unwrap_or("0.0.0.0:47881");
            if let Err(err) = serve_companion_daemon(bind, &data_root, &compatdata) {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        Some("--map-proton") => {
            let app_id: u32 = args
                .get(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let rel = args.get(2).map(String::as_str).unwrap_or(".duke-profile/documents/save.dat");
            match map_duke_profile_rel_to_proton(&compatdata, app_id, rel) {
                Ok(p) => println!("{}", p.display()),
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
        }
        Some("--hash") => {
            let Some(file_arg) = args.get(1) else {
                eprintln!("Missing <file_path>");
                std::process::exit(2);
            };
            match hash_file_blake3_streaming(Path::new(file_arg)) {
                Ok((hex, size)) => println!("{}  {} ({} bytes)", hex, file_arg, size),
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
        }
        Some("--status") => {
            let info = serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "os": std::env::consts::OS,
                "tailscaleIp": detect_tailscale_ipv4(),
                "compatdataRoot": compatdata,
                "defaultTcpPort": DEFAULT_COMPANION_PORT,
            });
            let _ = BTreeMap::<String, String>::new();
            println!("{info}");
        }
        _ => print_usage(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proton_compatdata_mapping_bidirectional() {
        let compat = Path::new("/steam/steamapps/compatdata");
        let mapped = map_duke_profile_rel_to_proton(
            compat,
            1245620,
            ".duke-profile/appdata/Roaming/EldenRing/ER0000.sl2",
        )
        .expect("map appdata roaming");
        let s = mapped.to_string_lossy().replace('\\', "/");
        assert!(s.ends_with("1245620/pfx/drive_c/users/steamuser/AppData/Roaming/EldenRing/ER0000.sl2"));

        let rev = map_proton_user_rel_to_duke_profile("Saved Games/HollowKnight/user1.dat")
            .expect("reverse map");
        assert_eq!(rev, ".duke-profile/saved_games/HollowKnight/user1.dat");

        assert!(map_duke_profile_rel_to_proton(compat, 123, "../../etc/passwd").is_err());
    }

    #[test]
    fn tailscale_cgnat_100_64_validation() {
        assert!(is_tailscale_cgnat_v4(Ipv4Addr::new(100, 64, 0, 1)));
        assert!(is_tailscale_cgnat_v4(Ipv4Addr::new(100, 101, 42, 99)));
        assert!(is_tailscale_cgnat_v4(Ipv4Addr::new(100, 127, 255, 254)));
        assert!(!is_tailscale_cgnat_v4(Ipv4Addr::new(100, 128, 0, 1)));
        assert!(!is_tailscale_cgnat_v4(Ipv4Addr::new(192, 168, 1, 10)));
    }

    #[test]
    fn streaming_64kb_blake3_chunk_verification() {
        let tmp_dir = env::temp_dir().join(format!("duke_comp_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&tmp_dir);
        let save_file = tmp_dir.join("test_save.bin");
        let payload = vec![0xA5u8; 192 * 1024]; // 3 full 64 KiB chunks
        fs::write(&save_file, &payload).expect("write test file");

        let (hex, size) = hash_file_blake3_streaming(&save_file).expect("hash file");
        assert_eq!(size, 192 * 1024);
        assert_eq!(hex, blake3::hash(&payload).to_hex().to_string());

        let zst = zstd::encode_all(payload.as_slice(), 3).expect("compress");
        let verified = decompress_and_verify_blake3(&zst, &hex).expect("verify blake3");
        assert_eq!(verified.len(), payload.len());
        assert!(decompress_and_verify_blake3(&zst, "00000000").is_err());

        let _ = fs::remove_dir_all(&tmp_dir);
    }
}
