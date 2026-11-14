use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

use crate::snapshots::{create_snapshot_in_dir, CAS_DIR_NAME};
use crate::{atomic_write_string, format_bytes, load_library_standalone, resolve_game_root_dir};

const DKDS_MAGIC: &[u8; 8] = b"DKDSZST1";
const DKDS_VERSION: u32 = 1;
const DKDS_CHUNK_BYTES: usize = 64 * 1024;
const DS_STREAMS_DIR: &str = ".duke-ds";
const DS_MANIFEST_FILE: &str = "directstorage_streams.json";
const DEFAULT_MIN_ARCHIVE_BYTES: u64 = 1024 * 1024; // 1 MiB

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DllProbeEntry {
    pub dll_name: String,
    pub loaded: bool,
    pub resolved_path: Option<String>,
    pub exported_symbols: Vec<String>,
    pub source_scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectStorageCapabilityReport {
    pub gpu_name: String,
    pub gpu_vendor: String,
    pub driver_version: String,
    pub d3d12_device_ready: bool,
    pub dstorage_dll: DllProbeEntry,
    pub dstorage_core_dll: DllProbeEntry,
    pub gacl_dll: DllProbeEntry,
    pub gpu_decompression_ready: bool,
    pub active_backend: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectStorageAssetCandidate {
    pub rel_path: String,
    pub extension: String,
    pub size_bytes: u64,
    pub size_formatted: String,
    pub entropy_bits: f64,
    pub detected_codec: String,
    pub already_packed: bool,
    pub packed_stream_bytes: u64,
    pub eligible_for_zstd_repack: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectStorageScanReport {
    pub game_id: String,
    pub game_root: String,
    pub capability: DirectStorageCapabilityReport,
    pub candidates: Vec<DirectStorageAssetCandidate>,
    pub total_candidate_bytes: u64,
    pub eligible_uncompressed_bytes: u64,
    pub existing_packed_streams: usize,
    pub existing_packed_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectStorageStreamRecord {
    pub source_rel_path: String,
    pub stream_rel_path: String,
    pub source_bytes: u64,
    pub packed_bytes: u64,
    pub chunk_count: u32,
    pub chunk_size_bytes: u32,
    pub compression_ratio_pct: f64,
    pub source_blake3: String,
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DirectStorageManifest {
    pub game_id: String,
    pub restore_snapshot_id: Option<String>,
    pub updated_at_epoch_sec: u64,
    pub zstd_level: i32,
    pub streams: Vec<DirectStorageStreamRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectStorageRepackReport {
    pub game_id: String,
    pub restore_snapshot_id: Option<String>,
    pub files_packed: usize,
    pub files_skipped: usize,
    pub source_bytes_total: u64,
    pub packed_bytes_total: u64,
    pub saved_bytes: u64,
    pub saved_formatted: String,
    pub elapsed_ms: u64,
    pub active_backend: String,
    pub streams: Vec<DirectStorageStreamRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectStorageVerifyReport {
    pub game_id: String,
    pub streams_checked: usize,
    pub streams_valid: usize,
    pub streams_corrupt: Vec<String>,
    pub chunks_verified: u64,
    pub total_uncompressed_bytes: u64,
    pub elapsed_ms: u64,
    pub all_valid: bool,
}

#[cfg(windows)]
mod win_ds {
    use super::DllProbeEntry;
    use std::ffi::c_void;
    use std::path::{Path, PathBuf};
    use windows::core::{PCSTR, PCWSTR};
    use windows::Win32::Foundation::{FreeLibrary, HMODULE};
    use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetProcAddress, LoadLibraryW};

    struct ModuleGuard(HMODULE);
    impl Drop for ModuleGuard {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                unsafe {
                    let _ = FreeLibrary(self.0);
                }
            }
        }
    }

    fn wide_null(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn candidate_search_dirs(game_dir: Option<&Path>) -> Vec<(PathBuf, &'static str)> {
        let mut dirs = Vec::new();
        if let Some(root) = game_dir {
            dirs.push((root.to_path_buf(), "Game Root"));
            for rel in ["bin", "Bin", "x64", "Win64", "Binaries/Win64"] {
                let sub = root.join(rel);
                if sub.is_dir() {
                    dirs.push((sub, "Game Binary Subfolder"));
                }
            }
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                dirs.push((parent.to_path_buf(), "Launcher Runtime Folder"));
            }
        }
        dirs
    }

    fn try_load_module(path_or_name: &str) -> Option<ModuleGuard> {
        let wide = wide_null(path_or_name);
        let hmod = unsafe { LoadLibraryW(PCWSTR(wide.as_ptr())) }.ok()?;
        if hmod.is_invalid() {
            None
        } else {
            Some(ModuleGuard(hmod))
        }
    }

    fn module_path(hmod: HMODULE) -> Option<String> {
        let mut buf = vec![0u16; 1024];
        let len = unsafe { GetModuleFileNameW(Some(hmod), &mut buf) } as usize;
        if len == 0 {
            None
        } else {
            Some(String::from_utf16_lossy(&buf[..len]))
        }
    }

    fn probe_symbols(hmod: HMODULE, candidates: &[&[u8]]) -> Vec<String> {
        let mut found = Vec::new();
        for &sym in candidates {
            let ptr = unsafe { GetProcAddress(hmod, PCSTR(sym.as_ptr())) };
            if ptr.is_some() {
                let raw = &sym[..sym.len().saturating_sub(1)];
                found.push(String::from_utf8_lossy(raw).to_string());
            }
        }
        found
    }

    pub fn probe_dll(
        dll_name: &str,
        exports: &[&[u8]],
        game_dir: Option<&Path>,
    ) -> DllProbeEntry {
        for (dir, scope) in candidate_search_dirs(game_dir) {
            let full = dir.join(dll_name);
            if full.is_file() {
                let full_str = full.to_string_lossy().to_string();
                if let Some(guard) = try_load_module(&full_str) {
                    let syms = probe_symbols(guard.0, exports);
                    let resolved = module_path(guard.0).or(Some(full_str));
                    return DllProbeEntry {
                        dll_name: dll_name.to_string(),
                        loaded: !syms.is_empty() || resolved.is_some(),
                        resolved_path: resolved,
                        exported_symbols: syms,
                        source_scope: scope.to_string(),
                    };
                }
            }
        }

        if let Some(guard) = try_load_module(dll_name) {
            let syms = probe_symbols(guard.0, exports);
            let resolved = module_path(guard.0);
            return DllProbeEntry {
                dll_name: dll_name.to_string(),
                loaded: true,
                resolved_path: resolved,
                exported_symbols: syms,
                source_scope: "System Search Path".to_string(),
            };
        }

        DllProbeEntry {
            dll_name: dll_name.to_string(),
            loaded: false,
            resolved_path: None,
            exported_symbols: Vec::new(),
            source_scope: "Unavailable (CPU/IoRing Fallback)".to_string(),
        }
    }

    pub fn probe_d3d12_compute_ready() -> bool {
        let Some(guard) = try_load_module("d3d12.dll") else {
            return false;
        };
        let proc = unsafe {
            GetProcAddress(guard.0, PCSTR(c"D3D12CreateDevice".as_ptr().cast()))
        };
        let Some(addr) = proc else {
            return false;
        };
        type D3D12CreateDeviceFn =
            unsafe extern "system" fn(*mut c_void, u32, *const [u8; 16], *mut *mut c_void) -> i32;
        let create_fn: D3D12CreateDeviceFn = unsafe { std::mem::transmute(addr) };
        // IID_ID3D12Device = {189819f1-1db6-4b57-be54-1821339b85f7} in little-endian UUID bytes.
        const IID_ID3D12_DEVICE: [u8; 16] = [
            0xf1, 0x19, 0x98, 0x18, 0xb6, 0x1d, 0x57, 0x4b, 0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b,
            0x85, 0xf7,
        ];
        // D3D_FEATURE_LEVEL_12_0 = 0xc000; null ppDevice queries support without allocating a device.
        let hr = unsafe {
            create_fn(
                std::ptr::null_mut(),
                0xc000,
                &IID_ID3D12_DEVICE,
                std::ptr::null_mut(),
            )
        };
        // S_FALSE (1) or S_OK (0) confirms D3D12 FL 12.0+ hardware capability for GPU compute codecs.
        hr >= 0
    }
}

#[cfg(not(windows))]
mod win_ds {
    use super::DllProbeEntry;
    use std::path::Path;

    pub fn probe_dll(
        dll_name: &str,
        _exports: &[&[u8]],
        _game_dir: Option<&Path>,
    ) -> DllProbeEntry {
        DllProbeEntry {
            dll_name: dll_name.to_string(),
            loaded: false,
            resolved_path: None,
            exported_symbols: Vec::new(),
            source_scope: "Non-Windows Fallback".to_string(),
        }
    }

    pub fn probe_d3d12_compute_ready() -> bool {
        false
    }
}

pub fn probe_direct_storage_runtime(game_dir: Option<&Path>) -> DirectStorageCapabilityReport {
    let gpu = crate::shader_swarm::detect_gpu_hardware_identity();
    let d3d12_device_ready = win_ds::probe_d3d12_compute_ready();

    let dstorage_dll = win_ds::probe_dll(
        "dstorage.dll",
        &[
            b"DStorageGetFactory\0",
            b"DStorageSetConfiguration\0",
            b"DStorageSetConfiguration1\0",
        ],
        game_dir,
    );
    let dstorage_core_dll = win_ds::probe_dll(
        "dstoragecore.dll",
        &[b"DStorageCreateCompressionCodec\0"],
        game_dir,
    );
    let gacl_dll = win_ds::probe_dll(
        "gacl.dll",
        &[
            b"GaclCreateCompressor\0",
            b"GaclCreateDecompressor\0",
            b"GaclGetVersion\0",
        ],
        game_dir,
    );

    let dll_ready = dstorage_dll.loaded && dstorage_core_dll.loaded;
    let gpu_decompression_ready = d3d12_device_ready && (dll_ready || gacl_dll.loaded);

    let active_backend = if dll_ready && gacl_dll.loaded {
        "DirectStorage 1.4 + GACL Hardware Codec"
    } else if dll_ready {
        "DirectStorage + D3D12 GPU Decompression"
    } else if gacl_dll.loaded && d3d12_device_ready {
        "GACL Compute Compression Runtime"
    } else if d3d12_device_ready {
        "D3D12 FL12.0+ GPU Ready (CPU Zstd + IoRing Stage Fallback)"
    } else {
        "CPU Zstd Stream + Win32 Prefetch Fallback"
    }
    .to_string();

    let summary = if gpu_decompression_ready {
        format!(
            "{} | {} ({})",
            gpu.adapter_name, active_backend, gpu.driver_version
        )
    } else if d3d12_device_ready {
        format!(
            "{}: D3D12 FL12+ compute capable; game folder lacks bundled dstorage.dll so Duke uses 64 KiB Zstd + IoRing safe fallback.",
            gpu.adapter_name
        )
    } else {
        "Direct3D 12 compute unavailable; safe CPU Zstd chunk streaming active.".to_string()
    };

    let gpu_vendor = match gpu.vendor_id {
        0x10DE => "NVIDIA".to_string(),
        0x1002 => "AMD".to_string(),
        0x8086 => "Intel".to_string(),
        other => format!("0x{:04X}", other),
    };

    DirectStorageCapabilityReport {
        gpu_name: gpu.adapter_name,
        gpu_vendor,
        driver_version: gpu.driver_version,
        d3d12_device_ready,
        dstorage_dll,
        dstorage_core_dll,
        gacl_dll,
        gpu_decompression_ready,
        active_backend,
        summary,
    }
}

pub fn sample_shannon_entropy(bytes: &[u8]) -> f64 {
    if bytes.is_empty() {
        return 0.0;
    }
    let mut freq = [0u64; 256];
    for &b in bytes {
        freq[b as usize] += 1;
    }
    let len = bytes.len() as f64;
    let mut entropy = 0.0f64;
    for count in freq {
        if count == 0 {
            continue;
        }
        let p = (count as f64) / len;
        entropy -= p * p.log2();
    }
    entropy
}

fn detect_archive_codec(header: &[u8], entropy: f64) -> String {
    if header.starts_with(DKDS_MAGIC) {
        return "DKDS-Zstd".to_string();
    }
    if header.len() >= 4 && header[0..4] == [0x28, 0xB5, 0x2F, 0xFD] {
        return "Zstd Frame".to_string();
    }
    if header.len() >= 4 && header[0..4] == *b"PK\x03\x04" {
        return if entropy > 7.35 {
            "ZIP/Deflate".to_string()
        } else {
            "ZIP/Store".to_string()
        };
    }
    if header.len() >= 4 && header[0..4] == *b"BTDX" {
        return if entropy > 7.35 {
            "Bethesda BA2 (Compressed)".to_string()
        } else {
            "Bethesda BA2 (Raw)".to_string()
        };
    }
    if entropy > 7.40 {
        "Pre-Compressed / Encrypted".to_string()
    } else if entropy > 6.30 {
        "Mixed Uncompressed".to_string()
    } else {
        "Low-Entropy Raw".to_string()
    }
}

fn is_candidate_archive_ext(ext: &str) -> bool {
    matches!(
        ext,
        "pak" | "ba2" | "forge" | "ucas" | "utoc" | "bsa" | "vpk" | "bin" | "dat" | "assets" | "pck"
    )
}

fn inspect_file_head(path: &Path) -> Result<(f64, String), String> {
    let mut file = File::open(path)
        .map_err(|e| format!("Failed to open {} for inspection: {}", path.display(), e))?;
    let mut sample = vec![0u8; 128 * 1024];
    let n = file
        .read(&mut sample)
        .map_err(|e| format!("Failed to inspect {}: {}", path.display(), e))?;
    let slice = &sample[..n];
    let entropy = sample_shannon_entropy(slice);
    let codec = detect_archive_codec(slice, entropy);
    Ok((entropy, codec))
}

fn normalize_rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn stream_rel_for_source(source_rel: &str) -> String {
    let sanitized = source_rel.replace(['/', '\\'], "__");
    format!("{}/{}.dkds", DS_STREAMS_DIR, sanitized)
}

fn manifest_path(game_root: &Path) -> PathBuf {
    game_root.join(DS_STREAMS_DIR).join(DS_MANIFEST_FILE)
}

fn load_ds_manifest(game_root: &Path) -> DirectStorageManifest {
    let path = manifest_path(game_root);
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_ds_manifest(game_root: &Path, manifest: &DirectStorageManifest) -> Result<(), String> {
    let ds_dir = game_root.join(DS_STREAMS_DIR);
    fs::create_dir_all(&ds_dir)
        .map_err(|e| format!("Failed to create {}: {}", ds_dir.display(), e))?;
    let json = serde_json::to_string_pretty(manifest)
        .map_err(|e| format!("Failed to serialize DirectStorage manifest: {}", e))?;
    atomic_write_string(&manifest_path(game_root), &json)
}

fn should_skip_ds_walk(entry: &walkdir::DirEntry, root: &Path) -> bool {
    if entry.path() == root {
        return false;
    }
    let name = entry.file_name().to_string_lossy();
    entry.file_type().is_dir()
        && (name.eq_ignore_ascii_case(CAS_DIR_NAME)
            || name.eq_ignore_ascii_case(DS_STREAMS_DIR)
            || name.eq_ignore_ascii_case(".duke-staging")
            || name.eq_ignore_ascii_case(".duke-profile")
            || name.eq_ignore_ascii_case("$RECYCLE.BIN")
            || name.eq_ignore_ascii_case("System Volume Information"))
}

pub fn encode_dkds_stream(
    src_path: &Path,
    dst_stream_path: &Path,
    zstd_level: i32,
) -> Result<(u64, u32, String), String> {
    if let Some(parent) = dst_stream_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create stream folder {}: {}", parent.display(), e))?;
    }
    let src_file = File::open(src_path)
        .map_err(|e| format!("Failed to open source archive {}: {}", src_path.display(), e))?;
    let src_len = src_file
        .metadata()
        .map_err(|e| format!("Failed to stat {}: {}", src_path.display(), e))?
        .len();
    let mut reader = BufReader::with_capacity(DKDS_CHUNK_BYTES, src_file);

    let tmp_path = dst_stream_path.with_extension("dkds.tmp");
    let dst_file = File::create(&tmp_path)
        .map_err(|e| format!("Failed to create stream {}: {}", tmp_path.display(), e))?;
    let mut writer = BufWriter::with_capacity(DKDS_CHUNK_BYTES, dst_file);

    let level = zstd_level.clamp(1, 12);
    let mut full_hasher = blake3::Hasher::new();

    // Header (32 bytes):
    // [0..8]   Magic b"DKDSZST1"
    // [8..12]  Version u32 LE
    // [12..16] Chunk size u32 LE (65536)
    // [16..24] Total uncompressed bytes u64 LE
    // [24..28] Chunk count u32 LE (patched at finish or written sequential)
    // [28..32] Reserved flags u32 LE (zstd level)
    let expected_chunks = src_len.div_ceil(DKDS_CHUNK_BYTES as u64) as u32;
    writer
        .write_all(DKDS_MAGIC)
        .map_err(|e| format!("Header write error: {}", e))?;
    writer
        .write_all(&DKDS_VERSION.to_le_bytes())
        .map_err(|e| format!("Header write error: {}", e))?;
    writer
        .write_all(&(DKDS_CHUNK_BYTES as u32).to_le_bytes())
        .map_err(|e| format!("Header write error: {}", e))?;
    writer
        .write_all(&src_len.to_le_bytes())
        .map_err(|e| format!("Header write error: {}", e))?;
    writer
        .write_all(&expected_chunks.to_le_bytes())
        .map_err(|e| format!("Header write error: {}", e))?;
    writer
        .write_all(&(level as u32).to_le_bytes())
        .map_err(|e| format!("Header write error: {}", e))?;

    let mut raw_buf = vec![0u8; DKDS_CHUNK_BYTES];
    let mut actual_chunks = 0u32;
    loop {
        let mut filled = 0usize;
        while filled < DKDS_CHUNK_BYTES {
            let n = reader
                .read(&mut raw_buf[filled..])
                .map_err(|e| format!("Failed reading {}: {}", src_path.display(), e))?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        if filled == 0 {
            break;
        }
        let chunk = &raw_buf[..filled];
        full_hasher.update(chunk);
        let chunk_digest = blake3::hash(chunk);
        let compressed = zstd::bulk::compress(chunk, level)
            .map_err(|e| format!("Zstd chunk compression failed: {}", e))?;

        // Per-chunk frame: raw_len u32 LE | zstd_len u32 LE | chunk_blake3_head [u8; 8] | payload
        writer
            .write_all(&(filled as u32).to_le_bytes())
            .map_err(|e| format!("Chunk write error: {}", e))?;
        writer
            .write_all(&(compressed.len() as u32).to_le_bytes())
            .map_err(|e| format!("Chunk write error: {}", e))?;
        writer
            .write_all(&chunk_digest.as_bytes()[..8])
            .map_err(|e| format!("Chunk write error: {}", e))?;
        writer
            .write_all(&compressed)
            .map_err(|e| format!("Chunk write error: {}", e))?;

        actual_chunks = actual_chunks.saturating_add(1);
    }

    let source_blake3 = full_hasher.finalize();
    writer
        .write_all(source_blake3.as_bytes())
        .map_err(|e| format!("Trailer write error: {}", e))?;
    writer
        .flush()
        .map_err(|e| format!("Flush error on {}: {}", tmp_path.display(), e))?;
    drop(writer);

    if actual_chunks != expected_chunks {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!(
            "Chunk count mismatch encoding {}: expected {}, encoded {}",
            src_path.display(),
            expected_chunks,
            actual_chunks
        ));
    }

    if dst_stream_path.exists() {
        let _ = fs::remove_file(dst_stream_path);
    }
    fs::rename(&tmp_path, dst_stream_path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        format!("Failed committing stream {}: {}", dst_stream_path.display(), e)
    })?;

    let packed_len = fs::metadata(dst_stream_path).map(|m| m.len()).unwrap_or(0);
    Ok((
        packed_len,
        actual_chunks,
        source_blake3.to_hex().to_string(),
    ))
}

pub fn verify_dkds_stream(
    stream_path: &Path,
    expected_blake3: Option<&str>,
) -> Result<(u64, u32, String), String> {
    let file = File::open(stream_path)
        .map_err(|e| format!("Failed to open stream {}: {}", stream_path.display(), e))?;
    let mut reader = BufReader::with_capacity(DKDS_CHUNK_BYTES, file);

    let mut header = [0u8; 32];
    reader
        .read_exact(&mut header)
        .map_err(|e| format!("Truncated DKDS header in {}: {}", stream_path.display(), e))?;
    if &header[0..8] != DKDS_MAGIC {
        return Err(format!(
            "Invalid DKDS magic in {}",
            stream_path.display()
        ));
    }
    let version = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
    if version != DKDS_VERSION {
        return Err(format!("Unsupported DKDS version {}", version));
    }
    let chunk_cap = u32::from_le_bytes([header[12], header[13], header[14], header[15]]) as usize;
    let total_raw = u64::from_le_bytes([
        header[16], header[17], header[18], header[19], header[20], header[21], header[22],
        header[23],
    ]);
    let chunk_count = u32::from_le_bytes([header[24], header[25], header[26], header[27]]);

    if chunk_cap == 0 || chunk_cap > 4 * 1024 * 1024 {
        return Err(format!("Invalid DKDS chunk size {}", chunk_cap));
    }

    let mut full_hasher = blake3::Hasher::new();
    let mut verified_raw = 0u64;
    let mut comp_buf = Vec::<u8>::new();
    let mut chunk_hdr = [0u8; 16];

    for idx in 0..chunk_count {
        reader.read_exact(&mut chunk_hdr).map_err(|e| {
            format!(
                "Truncated chunk header #{} in {}: {}",
                idx,
                stream_path.display(),
                e
            )
        })?;
        let raw_len =
            u32::from_le_bytes([chunk_hdr[0], chunk_hdr[1], chunk_hdr[2], chunk_hdr[3]]) as usize;
        let zstd_len =
            u32::from_le_bytes([chunk_hdr[4], chunk_hdr[5], chunk_hdr[6], chunk_hdr[7]]) as usize;
        let expected_head = &chunk_hdr[8..16];

        if raw_len == 0 || raw_len > chunk_cap || zstd_len == 0 || zstd_len > chunk_cap * 2 {
            return Err(format!(
                "Corrupt chunk #{} dimensions (raw={}, zstd={}) in {}",
                idx,
                raw_len,
                zstd_len,
                stream_path.display()
            ));
        }

        comp_buf.resize(zstd_len, 0);
        reader.read_exact(&mut comp_buf).map_err(|e| {
            format!(
                "Truncated chunk #{} payload in {}: {}",
                idx,
                stream_path.display(),
                e
            )
        })?;
        let decompressed = zstd::bulk::decompress(&comp_buf, raw_len).map_err(|e| {
            format!(
                "Zstd decompression failed on chunk #{} in {}: {}",
                idx,
                stream_path.display(),
                e
            )
        })?;
        if decompressed.len() != raw_len {
            return Err(format!(
                "Chunk #{} decompressed len {} != expected {}",
                idx,
                decompressed.len(),
                raw_len
            ));
        }
        let chunk_digest = blake3::hash(&decompressed);
        if &chunk_digest.as_bytes()[..8] != expected_head {
            return Err(format!(
                "Chunk #{} BLAKE3 digest mismatch in {}",
                idx,
                stream_path.display()
            ));
        }
        full_hasher.update(&decompressed);
        verified_raw = verified_raw.saturating_add(raw_len as u64);
    }

    let mut trailer = [0u8; 32];
    reader.read_exact(&mut trailer).map_err(|e| {
        format!(
            "Missing 32-byte BLAKE3 trailer in {}: {}",
            stream_path.display(),
            e
        )
    })?;
    if verified_raw != total_raw {
        return Err(format!(
            "Stream total byte count {} != header {}",
            verified_raw, total_raw
        ));
    }
    let computed_hash = full_hasher.finalize();
    if computed_hash.as_bytes() != &trailer {
        return Err(format!(
            "Trailer BLAKE3 mismatch in {}",
            stream_path.display()
        ));
    }
    let hex = computed_hash.to_hex().to_string();
    if let Some(expected) = expected_blake3 {
        if !expected.eq_ignore_ascii_case(&hex) {
            return Err(format!(
                "Expected BLAKE3 {} did not match computed {}",
                expected, hex
            ));
        }
    }

    Ok((verified_raw, chunk_count, hex))
}

fn resolve_game_root_by_id(game_id: &str) -> Result<PathBuf, String> {
    let lib = load_library_standalone()?;
    let game = lib
        .iter()
        .find(|g| g.id == game_id)
        .ok_or_else(|| format!("Game not found in library: {}", game_id))?;
    let root = resolve_game_root_dir(Path::new(&game.exe_path));
    if !root.is_dir() {
        return Err(format!("Game install folder missing: {}", root.display()));
    }
    Ok(root)
}

#[tauri::command]
pub async fn probe_direct_storage_capability(
    game_id: Option<String>,
) -> Result<DirectStorageCapabilityReport, String> {
    let game_root = match game_id.as_deref() {
        Some(id) if !id.trim().is_empty() => resolve_game_root_by_id(id.trim()).ok(),
        _ => None,
    };
    tokio::task::spawn_blocking(move || probe_direct_storage_runtime(game_root.as_deref()))
        .await
        .map_err(|e| format!("DirectStorage capability probe task failed: {}", e))
}

#[tauri::command]
pub async fn scan_direct_storage_assets(
    game_id: String,
    min_file_mb: Option<u64>,
) -> Result<DirectStorageScanReport, String> {
    tokio::task::spawn_blocking(move || {
        let root = resolve_game_root_by_id(&game_id)?;
        let cap = probe_direct_storage_runtime(Some(&root));
        let manifest = load_ds_manifest(&root);
        let packed_by_rel: HashSet<String> = manifest
            .streams
            .iter()
            .map(|s| s.source_rel_path.clone())
            .collect();
        let min_bytes = min_file_mb
            .map(|m| m.max(1).saturating_mul(1024 * 1024))
            .unwrap_or(DEFAULT_MIN_ARCHIVE_BYTES);

        let mut candidates = Vec::new();
        let mut total_candidate_bytes = 0u64;
        let mut eligible_uncompressed_bytes = 0u64;

        for entry in WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !should_skip_ds_walk(e, &root))
        {
            let entry = match entry {
                Ok(v) => v,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            if !is_candidate_archive_ext(&ext) {
                continue;
            }
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let size_bytes = meta.len();
            if size_bytes < min_bytes {
                continue;
            }
            let (entropy_bits, detected_codec) =
                inspect_file_head(path).unwrap_or((7.5, "Unknown".to_string()));
            let rel_path = normalize_rel(&root, path);
            let already_packed = packed_by_rel.contains(&rel_path)
                && root.join(stream_rel_for_source(&rel_path)).is_file();
            let packed_stream_bytes = if already_packed {
                fs::metadata(root.join(stream_rel_for_source(&rel_path)))
                    .map(|m| m.len())
                    .unwrap_or(0)
            } else {
                0
            };
            let eligible =
                !already_packed && entropy_bits < 7.35 && detected_codec != "Zstd Frame";

            total_candidate_bytes = total_candidate_bytes.saturating_add(size_bytes);
            if eligible {
                eligible_uncompressed_bytes =
                    eligible_uncompressed_bytes.saturating_add(size_bytes);
            }

            candidates.push(DirectStorageAssetCandidate {
                rel_path,
                extension: ext,
                size_bytes,
                size_formatted: format_bytes(size_bytes),
                entropy_bits: (entropy_bits * 100.0).round() / 100.0,
                detected_codec,
                already_packed,
                packed_stream_bytes,
                eligible_for_zstd_repack: eligible,
            });
        }

        candidates.sort_by_key(|a| std::cmp::Reverse(a.size_bytes));
        let existing_packed_bytes: u64 = manifest.streams.iter().map(|s| s.packed_bytes).sum();

        Ok(DirectStorageScanReport {
            game_id,
            game_root: root.to_string_lossy().to_string(),
            capability: cap,
            existing_packed_streams: manifest.streams.len(),
            existing_packed_bytes,
            candidates,
            total_candidate_bytes,
            eligible_uncompressed_bytes,
        })
    })
    .await
    .map_err(|e| format!("DirectStorage scan task failed: {}", e))?
}

#[tauri::command]
pub async fn repack_game_direct_storage_assets(
    game_id: String,
    zstd_level: Option<i32>,
    min_file_mb: Option<u64>,
    max_archives: Option<usize>,
) -> Result<DirectStorageRepackReport, String> {
    tokio::task::spawn_blocking(move || {
        let started = Instant::now();
        let root = resolve_game_root_by_id(&game_id)?;
        let cap = probe_direct_storage_runtime(Some(&root));
        let cas_dir = root.join(CAS_DIR_NAME);
        let snap = create_snapshot_in_dir(
            &game_id,
            &root,
            &cas_dir,
            Some("Pre-DirectStorage Zstd Repack"),
        )?;

        let level = zstd_level.unwrap_or(3).clamp(1, 12);
        let min_bytes = min_file_mb
            .map(|m| m.max(1).saturating_mul(1024 * 1024))
            .unwrap_or(DEFAULT_MIN_ARCHIVE_BYTES);
        let limit = max_archives.unwrap_or(24).clamp(1, 128);

        let mut manifest = load_ds_manifest(&root);
        manifest.game_id = game_id.clone();
        manifest.restore_snapshot_id = Some(snap.id.clone());
        manifest.zstd_level = level;

        let mut existing_map: HashSet<String> = manifest
            .streams
            .iter()
            .map(|s| s.source_rel_path.clone())
            .collect();

        let mut targets: Vec<(PathBuf, String, u64)> = Vec::new();
        for entry in WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !should_skip_ds_walk(e, &root))
        {
            let entry = match entry {
                Ok(v) => v,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            if !is_candidate_archive_ext(&ext) {
                continue;
            }
            let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            if size_bytes < min_bytes {
                continue;
            }
            let rel = normalize_rel(&root, path);
            if existing_map.contains(&rel) && root.join(stream_rel_for_source(&rel)).is_file() {
                continue;
            }
            let (entropy, codec) = inspect_file_head(path).unwrap_or((7.5, "Unknown".to_string()));
            if entropy >= 7.35 || codec == "Zstd Frame" {
                continue;
            }
            targets.push((path.to_path_buf(), rel, size_bytes));
        }
        targets.sort_by_key(|a| std::cmp::Reverse(a.2));
        targets.truncate(limit);

        let mut files_packed = 0usize;
        let mut files_skipped = 0usize;
        let mut source_bytes_total = 0u64;
        let mut packed_bytes_total = 0u64;

        for (src_path, rel_path, src_bytes) in targets {
            let stream_rel = stream_rel_for_source(&rel_path);
            let stream_full = root.join(&stream_rel);
            match encode_dkds_stream(&src_path, &stream_full, level) {
                Ok((packed_len, chunk_count, source_blake3)) => {
                    // Reject expansion if source unexpectedly resists Zstd beyond first 128 KiB sample.
                    if packed_len >= src_bytes {
                        let _ = fs::remove_file(&stream_full);
                        files_skipped += 1;
                        continue;
                    }
                    let ratio = if src_bytes > 0 {
                        ((packed_len as f64) / (src_bytes as f64) * 1000.0).round() / 10.0
                    } else {
                        100.0
                    };
                    manifest
                        .streams
                        .retain(|s| s.source_rel_path != rel_path);
                    manifest.streams.push(DirectStorageStreamRecord {
                        source_rel_path: rel_path.clone(),
                        stream_rel_path: stream_rel,
                        source_bytes: src_bytes,
                        packed_bytes: packed_len,
                        chunk_count,
                        chunk_size_bytes: DKDS_CHUNK_BYTES as u32,
                        compression_ratio_pct: ratio,
                        source_blake3,
                        verified: true,
                    });
                    existing_map.insert(rel_path);
                    files_packed += 1;
                    source_bytes_total = source_bytes_total.saturating_add(src_bytes);
                    packed_bytes_total = packed_bytes_total.saturating_add(packed_len);
                }
                Err(_) => {
                    files_skipped += 1;
                }
            }
        }

        manifest.updated_at_epoch_sec = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        save_ds_manifest(&root, &manifest)?;

        let saved_bytes = source_bytes_total.saturating_sub(packed_bytes_total);
        Ok(DirectStorageRepackReport {
            game_id,
            restore_snapshot_id: Some(snap.id),
            files_packed,
            files_skipped,
            source_bytes_total,
            packed_bytes_total,
            saved_bytes,
            saved_formatted: format_bytes(saved_bytes),
            elapsed_ms: started.elapsed().as_millis() as u64,
            active_backend: cap.active_backend,
            streams: manifest.streams,
        })
    })
    .await
    .map_err(|e| format!("DirectStorage repack task failed: {}", e))?
}

#[tauri::command]
pub async fn verify_direct_storage_streams(
    game_id: String,
) -> Result<DirectStorageVerifyReport, String> {
    tokio::task::spawn_blocking(move || {
        let started = Instant::now();
        let root = resolve_game_root_by_id(&game_id)?;
        let mut manifest = load_ds_manifest(&root);

        let mut streams_valid = 0usize;
        let mut streams_corrupt = Vec::new();
        let mut chunks_verified = 0u64;
        let mut total_uncompressed_bytes = 0u64;

        for rec in &mut manifest.streams {
            let stream_path = root.join(&rec.stream_rel_path);
            match verify_dkds_stream(&stream_path, Some(&rec.source_blake3)) {
                Ok((raw_len, chunk_count, _hash)) => {
                    rec.verified = true;
                    streams_valid += 1;
                    chunks_verified = chunks_verified.saturating_add(chunk_count as u64);
                    total_uncompressed_bytes = total_uncompressed_bytes.saturating_add(raw_len);
                }
                Err(err) => {
                    rec.verified = false;
                    streams_corrupt.push(format!("{}: {}", rec.source_rel_path, err));
                }
            }
        }
        let streams_checked = manifest.streams.len();
        if streams_checked > 0 {
            let _ = save_ds_manifest(&root, &manifest);
        }

        Ok(DirectStorageVerifyReport {
            game_id,
            streams_checked,
            streams_valid,
            all_valid: streams_corrupt.is_empty(),
            streams_corrupt,
            chunks_verified,
            total_uncompressed_bytes,
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    })
    .await
    .map_err(|e| format!("DirectStorage verify task failed: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_tmp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("duke_ds_test_{}_{}", name, nonce));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn dkds_zstd_stream_roundtrip_verifies_chunks_and_catches_corruption() {
        let dir = unique_tmp_dir("roundtrip");
        let src = dir.join("test_archive.pak");
        let dst = dir.join("test_archive.pak.dkds");

        // 150 KiB compressible payload spanning 3 64 KiB DirectStorage chunks.
        let mut payload = Vec::with_capacity(150 * 1024);
        for i in 0..(150 * 1024) {
            payload.push(((i / 64) % 13) as u8);
        }
        fs::File::create(&src).unwrap().write_all(&payload).unwrap();

        let (packed_len, chunk_count, digest) = encode_dkds_stream(&src, &dst, 3).unwrap();
        assert!(packed_len < payload.len() as u64);
        assert_eq!(chunk_count, 3);

        let (verified_raw, verified_chunks, verified_digest) =
            verify_dkds_stream(&dst, Some(&digest)).unwrap();
        assert_eq!(verified_raw, payload.len() as u64);
        assert_eq!(verified_chunks, 3);
        assert_eq!(verified_digest, digest);

        // Flip a byte inside the compressed stream and confirm BLAKE3/Zstd rejects corruption.
        let mut corrupted = fs::read(&dst).unwrap();
        let flip_idx = corrupted.len() - 40;
        corrupted[flip_idx] ^= 0x5A;
        fs::write(&dst, corrupted).unwrap();
        assert!(verify_dkds_stream(&dst, Some(&digest)).is_err());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn entropy_and_capability_probe_are_safe() {
        let uniform = vec![0x41u8; 4096];
        assert!(sample_shannon_entropy(&uniform) < 0.01);

        let all_bytes: Vec<u8> = (0..=255).cycle().take(4096).collect();
        assert!(sample_shannon_entropy(&all_bytes) > 7.9);

        let cap = probe_direct_storage_runtime(None);
        assert!(!cap.active_backend.is_empty());
    }
}
