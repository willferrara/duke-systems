use crate::swarm::{known_peer_addrs_public, send_tcp_request_public, SwarmWireRequest, SwarmWireResponse};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
};

const DKSHD_MAGIC: &[u8; 8] = b"DKSHD001";
const SHADER_INDEX_FILE: &str = "shader_swarm_index.json";
const MAX_BUNDLE_RAW_BYTES: usize = 64 * 1024 * 1024;
const MAX_SINGLE_SHADER_FILE_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuHardwareIdentity {
    pub adapter_name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub gpu_key: String,
    pub driver_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderBundleMeta {
    pub bundle_key: String,
    pub game_id: String,
    pub game_title: String,
    pub game_blake3: String,
    pub gpu_key: String,
    pub driver_version: String,
    pub file_count: usize,
    pub raw_bytes: u64,
    pub compressed_bytes: u64,
    pub bundle_path: String,
    pub updated_at_secs: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShaderSwarmIndex {
    bundles: HashMap<String, ShaderBundleMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderSwarmStatus {
    pub hardware: GpuHardwareIdentity,
    pub cache_dirs: Vec<String>,
    pub local_bundles: Vec<ShaderBundleMeta>,
    pub peer_count: usize,
    pub driver_purge: duke_core::win32_ffi::ShaderCachePurgeReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderSyncReport {
    pub game_title: String,
    pub game_blake3: String,
    pub bundle_key: String,
    pub source_peer: Option<String>,
    pub injected_files: usize,
    pub bytes_written: u64,
    pub driver_purged_files: usize,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackedShaderEntry {
    pub bucket: String,
    pub rel_path: String,
    pub data: Vec<u8>,
}

static SHADER_INDEX_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn index_lock() -> &'static Mutex<()> {
    SHADER_INDEX_LOCK.get_or_init(|| Mutex::new(()))
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn shader_store_dir() -> PathBuf {
    crate::get_app_data_file_standalone("shader_swarm")
        .unwrap_or_else(|_| std::env::temp_dir().join("duke-launcher-shader-swarm"))
}

fn load_shader_index() -> ShaderSwarmIndex {
    let _g = index_lock().lock().unwrap();
    let Ok(path) = crate::get_app_data_file_standalone(SHADER_INDEX_FILE) else {
        return ShaderSwarmIndex::default();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_shader_index(idx: &ShaderSwarmIndex) -> Result<(), String> {
    let _g = index_lock().lock().unwrap();
    let path = crate::get_app_data_file_standalone(SHADER_INDEX_FILE)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed creating shader index dir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(idx)
        .map_err(|e| format!("Failed serializing shader index: {e}"))?;
    fs::write(&path, json).map_err(|e| format!("Failed writing shader index: {e}"))
}

/// Detects primary DXGI hardware GPU `VendorId`, `DeviceId`, and driver version string.
pub fn detect_gpu_hardware_identity() -> GpuHardwareIdentity {
    let driver_version = duke_core::win32_ffi::detect_primary_gpu_driver_version()
        .unwrap_or_else(|| "unknown-driver".to_string());

    #[cfg(target_os = "windows")]
    {
        if let Ok(factory) = unsafe { CreateDXGIFactory1::<IDXGIFactory1>() } {
            let mut fallback: Option<(String, u32, u32)> = None;
            for idx in 0..16 {
                let Ok(adapter) = (unsafe { factory.EnumAdapters1(idx) }) else {
                    break;
                };
                let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
                    continue;
                };
                let end = desc
                    .Description
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(desc.Description.len());
                let name = String::from_utf16_lossy(&desc.Description[..end]).trim().to_string();
                let is_sw = (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0;
                if !is_sw {
                    return GpuHardwareIdentity {
                        adapter_name: name,
                        vendor_id: desc.VendorId,
                        device_id: desc.DeviceId,
                        gpu_key: format!("{:04x}-{:04x}", desc.VendorId, desc.DeviceId),
                        driver_version,
                    };
                }
                if fallback.is_none() {
                    fallback = Some((name, desc.VendorId, desc.DeviceId));
                }
            }
            if let Some((name, vid, did)) = fallback {
                return GpuHardwareIdentity {
                    adapter_name: name,
                    vendor_id: vid,
                    device_id: did,
                    gpu_key: format!("{vid:04x}-{did:04x}"),
                    driver_version,
                };
            }
        }
    }

    GpuHardwareIdentity {
        adapter_name: "Unknown GPU Adapter".to_string(),
        vendor_id: 0,
        device_id: 0,
        gpu_key: "0000-0000".to_string(),
        driver_version,
    }
}

pub fn make_shader_bundle_key(
    game_blake3: &str,
    vendor_id: u32,
    device_id: u32,
    driver_version: &str,
) -> String {
    let ghash: String = game_blake3
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .take(16)
        .collect::<String>()
        .to_ascii_lowercase();
    let d_clean: String = driver_version
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!(
        "{}_{:04x}-{:04x}_{}",
        if ghash.is_empty() { "unknown" } else { &ghash },
        vendor_id,
        device_id,
        d_clean.trim_matches('_')
    )
}

pub fn hash_file_blake3(path: &Path) -> Result<String, String> {
    let mut file = File::open(path)
        .map_err(|e| format!("Failed opening {} for BLAKE3 hash: {e}", path.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 256 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("Failed reading {} for BLAKE3 hash: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// Discovers vendor DX/Vulkan + emulator shader cache buckets dynamically via env vars.
pub fn discover_shader_cache_buckets(game_dir: Option<&Path>) -> Vec<(String, PathBuf)> {
    let mut buckets: Vec<(String, PathBuf)> = Vec::new();
    let local_app = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let app_data = std::env::var_os("APPDATA").map(PathBuf::from);
    let user_prof = std::env::var_os("USERPROFILE").map(PathBuf::from);

    if let Some(ref l) = local_app {
        buckets.push(("nv_dxcache".to_string(), l.join("NVIDIA").join("DXCache")));
        buckets.push(("nv_glcache".to_string(), l.join("NVIDIA").join("GLCache")));
        buckets.push(("d3d_scache".to_string(), l.join("D3DSCache")));
        buckets.push(("amd_dxcache".to_string(), l.join("AMD").join("DxCache")));
        buckets.push(("amd_vkcache".to_string(), l.join("AMD").join("VkCache")));
        buckets.push(("intel_scache".to_string(), l.join("Intel").join("ShaderCache")));
        buckets.push(("cemu_local".to_string(), l.join("Cemu").join("shaderCache")));
        buckets.push(("azahar_local".to_string(), l.join("Azahar").join("shaders")));
    }
    if let Some(ref r) = app_data {
        buckets.push(("dolphin_roam".to_string(), r.join("Dolphin Emulator").join("Cache")));
        buckets.push(("cemu_roam".to_string(), r.join("Cemu").join("shaderCache")));
        buckets.push(("pcsx2_roam".to_string(), r.join("PCSX2").join("cache")));
        buckets.push(("azahar_roam".to_string(), r.join("Azahar").join("shaders")));
        buckets.push((
            "ppsspp_roam".to_string(),
            r.join("PPSSPP").join("PSP").join("SYSTEM").join("CACHE"),
        ));
    }
    if let Some(ref u) = user_prof {
        buckets.push((
            "pcsx2_docs".to_string(),
            u.join("Documents").join("PCSX2").join("cache"),
        ));
    }
    if let Ok(emu_root) = crate::emulators::emulators_root_dir() {
        buckets.push((
            "dolphin_port".to_string(),
            emu_root.join("dolphin").join("User").join("Cache"),
        ));
        buckets.push((
            "cemu_port".to_string(),
            emu_root.join("cemu").join("shaderCache"),
        ));
        buckets.push((
            "pcsx2_port".to_string(),
            emu_root.join("pcsx2").join("cache"),
        ));
        buckets.push((
            "azahar_port".to_string(),
            emu_root.join("azahar").join("user").join("shaders"),
        ));
        buckets.push((
            "ppsspp_port".to_string(),
            emu_root
                .join("ppsspp")
                .join("memstick")
                .join("PSP")
                .join("SYSTEM")
                .join("CACHE"),
        ));
    }
    if let Some(gdir) = game_dir {
        buckets.push(("game_root".to_string(), gdir.to_path_buf()));
        buckets.push(("game_scache".to_string(), gdir.join("ShaderCache")));
    }

    buckets
}

fn is_shader_cache_artifact(bucket: &str, path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if bucket == "game_root" {
        return ext == "dxvk-cache"
            || ext == "vkpipeline"
            || name.contains("shader") && matches!(ext.as_str(), "bin" | "cache" | "upipelinecache" | "foz");
    }
    matches!(
        ext.as_str(),
        "bin"
            | "toc"
            | "uidcache"
            | "cache"
            | "dxvk-cache"
            | "vkpipeline"
            | "upipelinecache"
            | "foz"
            | "ushaderprecache"
            | "nv"
            | "spv"
    ) || name.ends_with(".bin")
        || !ext.is_empty()
}

fn safe_rel_join(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let clean = rel.replace('\\', "/");
    let mut out = root.to_path_buf();
    for comp in Path::new(&clean).components() {
        match comp {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return Err(format!("Blocked unsafe shader relative path: {rel}")),
        }
    }
    Ok(out)
}

pub fn encode_dkshd_bundle(entries: &[PackedShaderEntry]) -> Result<Vec<u8>, String> {
    let mut raw = Vec::new();
    raw.extend_from_slice(DKSHD_MAGIC);
    raw.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for e in entries {
        let bb = e.bucket.as_bytes();
        let rb = e.rel_path.as_bytes();
        raw.extend_from_slice(&(bb.len() as u16).to_le_bytes());
        raw.extend_from_slice(bb);
        raw.extend_from_slice(&(rb.len() as u16).to_le_bytes());
        raw.extend_from_slice(rb);
        raw.extend_from_slice(&(e.data.len() as u64).to_le_bytes());
        raw.extend_from_slice(&e.data);
    }
    zstd::bulk::compress(&raw, 6).map_err(|e| format!("Failed compressing DKSHD shader bundle: {e}"))
}

pub fn decode_dkshd_bundle(zst_bytes: &[u8]) -> Result<Vec<PackedShaderEntry>, String> {
    let raw = zstd::stream::decode_all(std::io::Cursor::new(zst_bytes))
        .map_err(|e| format!("Failed decompressing DKSHD shader bundle: {e}"))?;
    if raw.len() < 12 || &raw[0..8] != DKSHD_MAGIC {
        return Err("Invalid DKSHD001 shader bundle header".to_string());
    }
    let count = u32::from_le_bytes(raw[8..12].try_into().unwrap()) as usize;
    let mut cursor = 12usize;
    let mut entries = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        if cursor + 2 > raw.len() {
            return Err("Truncated DKSHD entry bucket length".to_string());
        }
        let blen = u16::from_le_bytes([raw[cursor], raw[cursor + 1]]) as usize;
        cursor += 2;
        if cursor + blen + 2 > raw.len() {
            return Err("Truncated DKSHD entry bucket".to_string());
        }
        let bucket = String::from_utf8(raw[cursor..cursor + blen].to_vec())
            .map_err(|_| "Invalid UTF-8 in shader bucket name".to_string())?;
        cursor += blen;
        let rlen = u16::from_le_bytes([raw[cursor], raw[cursor + 1]]) as usize;
        cursor += 2;
        if cursor + rlen + 8 > raw.len() {
            return Err("Truncated DKSHD entry path".to_string());
        }
        let rel_path = String::from_utf8(raw[cursor..cursor + rlen].to_vec())
            .map_err(|_| "Invalid UTF-8 in shader relative path".to_string())?;
        cursor += rlen;
        let dlen = u64::from_le_bytes(raw[cursor..cursor + 8].try_into().unwrap()) as usize;
        cursor += 8;
        if cursor + dlen > raw.len() {
            return Err("Truncated DKSHD shader payload".to_string());
        }
        let data = raw[cursor..cursor + dlen].to_vec();
        cursor += dlen;
        entries.push(PackedShaderEntry {
            bucket,
            rel_path,
            data,
        });
    }
    Ok(entries)
}

pub fn collect_and_archive_shaders_with_buckets(
    game_id: &str,
    game_title: &str,
    exe_path: &Path,
    since_epoch_secs: Option<u64>,
    buckets: &[(String, PathBuf)],
    store_dir: &Path,
    hw: &GpuHardwareIdentity,
) -> Result<ShaderBundleMeta, String> {
    let game_blake3 = if exe_path.is_file() {
        hash_file_blake3(exe_path)?
    } else {
        blake3::hash(game_id.as_bytes()).to_hex().to_string()
    };
    let bundle_key =
        make_shader_bundle_key(&game_blake3, hw.vendor_id, hw.device_id, &hw.driver_version);

    let mut entries: Vec<PackedShaderEntry> = Vec::new();
    let mut total_raw = 0usize;

    for (bucket_name, dir) in buckets {
        if !dir.is_dir() {
            continue;
        }
        for item in WalkDir::new(dir)
            .max_depth(3)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            if !item.file_type().is_file() || !is_shader_cache_artifact(bucket_name, item.path()) {
                continue;
            }
            let Ok(meta) = item.metadata() else {
                continue;
            };
            if meta.len() == 0 || meta.len() > MAX_SINGLE_SHADER_FILE_BYTES {
                continue;
            }
            if let Some(min_epoch) = since_epoch_secs {
                let is_game_local = bucket_name.starts_with("game_");
                let mtime_secs = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                // Capture vendor caches updated during this play session; always capture game-local caches.
                if !is_game_local && mtime_secs + 2 < min_epoch {
                    continue;
                }
            }
            if total_raw + meta.len() as usize > MAX_BUNDLE_RAW_BYTES {
                break;
            }
            let Ok(rel) = item.path().strip_prefix(dir) else {
                continue;
            };
            let rel_norm = rel.to_string_lossy().replace('\\', "/");
            if let Ok(data) = fs::read(item.path()) {
                total_raw += data.len();
                entries.push(PackedShaderEntry {
                    bucket: bucket_name.clone(),
                    rel_path: rel_norm,
                    data,
                });
            }
        }
    }

    fs::create_dir_all(store_dir)
        .map_err(|e| format!("Failed creating shader store {}: {e}", store_dir.display()))?;
    let encoded = encode_dkshd_bundle(&entries)?;
    let bundle_path = store_dir.join(format!("{bundle_key}.dkshd"));
    let mut f = File::create(&bundle_path)
        .map_err(|e| format!("Failed writing {}: {e}", bundle_path.display()))?;
    f.write_all(&encoded)
        .map_err(|e| format!("Failed flushing {}: {e}", bundle_path.display()))?;

    Ok(ShaderBundleMeta {
        bundle_key,
        game_id: game_id.to_string(),
        game_title: game_title.to_string(),
        game_blake3,
        gpu_key: hw.gpu_key.clone(),
        driver_version: hw.driver_version.clone(),
        file_count: entries.len(),
        raw_bytes: total_raw as u64,
        compressed_bytes: encoded.len() as u64,
        bundle_path: bundle_path.to_string_lossy().into_owned(),
        updated_at_secs: now_unix_secs(),
    })
}

pub fn inject_dkshd_bundle_into_buckets(
    zst_bytes: &[u8],
    buckets: &[(String, PathBuf)],
) -> Result<(usize, u64), String> {
    let entries = decode_dkshd_bundle(zst_bytes)?;
    let bucket_map: HashMap<&str, &PathBuf> =
        buckets.iter().map(|(k, v)| (k.as_str(), v)).collect();
    let mut injected = 0usize;
    let mut bytes_written = 0u64;

    for entry in entries {
        let Some(root) = bucket_map.get(entry.bucket.as_str()) else {
            continue;
        };
        let dst = safe_rel_join(root, &entry.rel_path)?;
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed creating shader target dir: {e}"))?;
        }
        fs::write(&dst, &entry.data)
            .map_err(|e| format!("Failed injecting shader {}: {e}", dst.display()))?;
        injected += 1;
        bytes_written = bytes_written.saturating_add(entry.data.len() as u64);
    }
    Ok((injected, bytes_written))
}

pub(crate) fn serve_shader_pull(
    game_blake3: &str,
    gpu_key: &str,
    driver_version: &str,
) -> SwarmWireResponse {
    let idx = load_shader_index();
    let short_hash = &game_blake3.chars().take(16).collect::<String>();
    let matched = idx.bundles.values().find(|b| {
        b.game_blake3.starts_with(short_hash)
            && b.gpu_key.eq_ignore_ascii_case(gpu_key)
            && b.driver_version.eq_ignore_ascii_case(driver_version)
    });
    if let Some(meta) = matched {
        if let Ok(bytes) = fs::read(&meta.bundle_path) {
            return SwarmWireResponse::ShaderBundle {
                found: true,
                bundle_zst_bytes: bytes,
                key: meta.bundle_key.clone(),
                file_count: meta.file_count,
            };
        }
    }
    SwarmWireResponse::ShaderBundle {
        found: false,
        bundle_zst_bytes: Vec::new(),
        key: String::new(),
        file_count: 0,
    }
}

pub fn capture_game_exit_shaders(
    game_id: &str,
    game_title: &str,
    exe_path: &Path,
    install_dir: &Path,
    since_epoch_secs: u64,
) -> Result<ShaderBundleMeta, String> {
    let hw = detect_gpu_hardware_identity();
    let buckets = discover_shader_cache_buckets(Some(install_dir));
    let meta = collect_and_archive_shaders_with_buckets(
        game_id,
        game_title,
        exe_path,
        Some(since_epoch_secs),
        &buckets,
        &shader_store_dir(),
        &hw,
    )?;
    if meta.file_count > 0 {
        let mut idx = load_shader_index();
        idx.bundles.insert(meta.bundle_key.clone(), meta.clone());
        save_shader_index(&idx)?;
    }
    Ok(meta)
}

pub async fn pull_and_inject_peer_shaders(
    game_id: &str,
    game_title: &str,
    exe_path: &Path,
    install_dir: &Path,
) -> Result<ShaderSyncReport, String> {
    let hw = detect_gpu_hardware_identity();
    let exe_clone = exe_path.to_path_buf();
    let gid_clone = game_id.to_string();
    let game_blake3 = tokio::task::spawn_blocking(move || {
        if exe_clone.is_file() {
            hash_file_blake3(&exe_clone)
        } else {
            Ok(blake3::hash(gid_clone.as_bytes()).to_hex().to_string())
        }
    })
    .await
    .map_err(|e| format!("Shader BLAKE3 task failed: {e}"))??;

    let app_root = crate::get_app_data_file_standalone("shader_swarm")?
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(shader_store_dir);
    let purge_res = duke_core::win32_ffi::check_and_purge_stale_vendor_shader_caches(
        &app_root,
        Some(&hw.driver_version),
    )
    .unwrap_or_default();

    let bundle_key =
        make_shader_bundle_key(&game_blake3, hw.vendor_id, hw.device_id, &hw.driver_version);
    let buckets = discover_shader_cache_buckets(Some(install_dir));

    for (peer_name, peer_addr) in known_peer_addrs_public() {
        let req = SwarmWireRequest::ShaderPull {
            game_blake3: game_blake3.clone(),
            gpu_key: hw.gpu_key.clone(),
            driver_version: hw.driver_version.clone(),
        };
        if let Ok(SwarmWireResponse::ShaderBundle {
            found: true,
            bundle_zst_bytes,
            key,
            file_count: _,
        }) = send_tcp_request_public(&peer_addr, &req).await
        {
            if !bundle_zst_bytes.is_empty() {
                let (injected_files, bytes_written) =
                    inject_dkshd_bundle_into_buckets(&bundle_zst_bytes, &buckets)?;
                return Ok(ShaderSyncReport {
                    game_title: game_title.to_string(),
                    game_blake3,
                    bundle_key: key,
                    source_peer: Some(peer_name),
                    injected_files,
                    bytes_written,
                    driver_purged_files: purge_res.files_removed,
                    summary: format!(
                        "Injected {} pre-compiled shader file(s) ({}) from Swarm peer for {} [{}].",
                        injected_files,
                        crate::format_bytes(bytes_written),
                        hw.adapter_name,
                        hw.gpu_key
                    ),
                });
            }
        }
    }

    Ok(ShaderSyncReport {
        game_title: game_title.to_string(),
        game_blake3,
        bundle_key,
        source_peer: None,
        injected_files: 0,
        bytes_written: 0,
        driver_purged_files: purge_res.files_removed,
        summary: format!(
            "No matching peer shader bundle for {} [{} / {}]; local shaders will archive on exit.",
            hw.adapter_name, hw.gpu_key, hw.driver_version
        ),
    })
}

#[tauri::command]
pub async fn get_shader_swarm_status() -> Result<ShaderSwarmStatus, String> {
    let hw = detect_gpu_hardware_identity();
    let app_root = crate::get_app_data_file_standalone("shader_swarm")?
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(shader_store_dir);
    let purge = duke_core::win32_ffi::check_and_purge_stale_vendor_shader_caches(
        &app_root,
        Some(&hw.driver_version),
    )
    .unwrap_or_default();
    let idx = load_shader_index();
    let mut local_bundles: Vec<ShaderBundleMeta> = idx.bundles.into_values().collect();
    local_bundles.sort_by_key(|a| std::cmp::Reverse(a.updated_at_secs));
    let cache_dirs = discover_shader_cache_buckets(None)
        .into_iter()
        .filter(|(_, p)| p.is_dir())
        .map(|(tag, p)| format!("{tag}: {}", p.display()))
        .collect();
    Ok(ShaderSwarmStatus {
        hardware: hw,
        cache_dirs,
        local_bundles,
        peer_count: known_peer_addrs_public().len(),
        driver_purge: purge,
    })
}

#[tauri::command]
pub async fn sync_game_shaders_from_swarm(
    app: tauri::AppHandle,
    game_id: String,
) -> Result<ShaderSyncReport, String> {
    let lib = crate::load_library_lightweight(&app)?;
    let entry = lib
        .into_iter()
        .find(|g| g.id == game_id)
        .ok_or_else(|| format!("Game not found in library: {game_id}"))?;
    pull_and_inject_peer_shaders(
        &entry.id,
        &entry.title,
        Path::new(&entry.exe_path),
        Path::new(&entry.install_dir),
    )
    .await
}

#[tauri::command]
pub async fn archive_game_shader_bundle(
    app: tauri::AppHandle,
    game_id: String,
) -> Result<ShaderBundleMeta, String> {
    let lib = crate::load_library_lightweight(&app)?;
    let entry = lib
        .into_iter()
        .find(|g| g.id == game_id)
        .ok_or_else(|| format!("Game not found in library: {game_id}"))?;
    tokio::task::spawn_blocking(move || {
        let hw = detect_gpu_hardware_identity();
        let buckets = discover_shader_cache_buckets(Some(Path::new(&entry.install_dir)));
        let meta = collect_and_archive_shaders_with_buckets(
            &entry.id,
            &entry.title,
            Path::new(&entry.exe_path),
            None,
            &buckets,
            &shader_store_dir(),
            &hw,
        )?;
        let mut idx = load_shader_index();
        idx.bundles.insert(meta.bundle_key.clone(), meta.clone());
        save_shader_index(&idx)?;
        Ok(meta)
    })
    .await
    .map_err(|e| format!("Archive shader bundle task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_bundle_key_and_dkshd_archive_round_trip() {
        let root = std::env::temp_dir().join(format!("duke_shader_swarm_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let src_dx = root.join("src_nv_dxcache");
        let src_dolphin = root.join("src_dolphin");
        let dst_dx = root.join("dst_nv_dxcache");
        let dst_dolphin = root.join("dst_dolphin");
        let store = root.join("store");
        fs::create_dir_all(&src_dx).unwrap();
        fs::create_dir_all(&src_dolphin).unwrap();
        fs::create_dir_all(&dst_dx).unwrap();
        fs::create_dir_all(&dst_dolphin).unwrap();

        let fake_exe = root.join("Game.exe");
        fs::write(&fake_exe, b"MZ-shader-swarm-binary-fixture").unwrap();
        fs::write(src_dx.join("dxcache01.bin"), vec![0xAAu8; 4096]).unwrap();
        fs::write(src_dolphin.join("GALE01.uidcache"), b"DOLPHIN_SHADER_UID").unwrap();

        let hw = GpuHardwareIdentity {
            adapter_name: "Test GPU".to_string(),
            vendor_id: 0x10DE,
            device_id: 0x2684,
            gpu_key: "10de-2684".to_string(),
            driver_version: "32.0.15.6636".to_string(),
        };
        let key = make_shader_bundle_key("abcdef0123456789ffff", hw.vendor_id, hw.device_id, &hw.driver_version);
        assert_eq!(key, "abcdef0123456789_10de-2684_32.0.15.6636");

        let src_buckets = vec![
            ("nv_dxcache".to_string(), src_dx.clone()),
            ("dolphin_roam".to_string(), src_dolphin.clone()),
        ];
        let meta = collect_and_archive_shaders_with_buckets(
            "g-test",
            "Test Game",
            &fake_exe,
            None,
            &src_buckets,
            &store,
            &hw,
        )
        .unwrap();
        assert_eq!(meta.file_count, 2);
        assert_eq!(meta.gpu_key, "10de-2684");
        assert!(Path::new(&meta.bundle_path).is_file());

        let zst_bytes = fs::read(&meta.bundle_path).unwrap();
        let dst_buckets = vec![
            ("nv_dxcache".to_string(), dst_dx.clone()),
            ("dolphin_roam".to_string(), dst_dolphin.clone()),
        ];
        let (injected, _) = inject_dkshd_bundle_into_buckets(&zst_bytes, &dst_buckets).unwrap();
        assert_eq!(injected, 2);
        assert_eq!(fs::read(dst_dx.join("dxcache01.bin")).unwrap(), vec![0xAAu8; 4096]);
        assert_eq!(fs::read(dst_dolphin.join("GALE01.uidcache")).unwrap(), b"DOLPHIN_SHADER_UID");

        // Block path traversal attempts in DKSHD entries.
        let evil = encode_dkshd_bundle(&[PackedShaderEntry {
            bucket: "nv_dxcache".to_string(),
            rel_path: "../../evil.dll".to_string(),
            data: b"x".to_vec(),
        }])
        .unwrap();
        assert!(inject_dkshd_bundle_into_buckets(&evil, &dst_buckets).is_err());

        let _ = fs::remove_dir_all(&root);
    }
}
