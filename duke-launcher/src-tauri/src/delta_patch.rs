use fastcdc::v2020::StreamCDC;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

use crate::{load_library_standalone, resolve_game_root_dir};

pub const FASTCDC_MIN_SIZE: u32 = 16 * 1024;
pub const FASTCDC_AVG_SIZE: u32 = 32 * 1024;
pub const FASTCDC_MAX_SIZE: u32 = 64 * 1024;

const DKMF_ZSTD_MAGIC: &[u8; 6] = b"DKMFZ1";
const DKDELTA_MAGIC: &[u8; 7] = b"DKDLT01";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkEntry {
    pub gear_hash: u64,
    pub offset: u64,
    pub length: u32,
    pub blake3: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestFileEntry {
    pub rel_path: String,
    pub size_bytes: u64,
    pub blake3: String,
    pub chunks: Vec<ChunkEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DkManifest {
    pub magic: String,
    pub root_label: String,
    pub created_at: u64,
    pub chunk_min: u32,
    pub chunk_avg: u32,
    pub chunk_max: u32,
    pub total_bytes: u64,
    pub files: Vec<ManifestFileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DkManifestSummary {
    pub manifest_path: String,
    pub file_count: usize,
    pub chunk_count: usize,
    pub total_bytes: u64,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeltaDiffReport {
    pub target_files: usize,
    pub total_target_bytes: u64,
    pub reused_chunks: usize,
    pub reused_bytes: u64,
    pub missing_chunks: usize,
    pub missing_bytes: u64,
    pub missing_hashes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeltaBundleStats {
    pub bundle_path: String,
    pub chunk_count: usize,
    pub uncompressed_bytes: u64,
    pub compressed_bytes: u64,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeltaApplyReport {
    pub files_patched: usize,
    pub files_unchanged: usize,
    pub files_removed: usize,
    pub chunks_reused_from_disk: usize,
    pub chunks_applied_from_delta: usize,
    pub bytes_stitched_total: u64,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone)]
pub struct LocalChunkLocation {
    pub file_path: PathBuf,
    pub offset: u64,
    pub length: u32,
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn should_skip_delta_entry(entry: &walkdir::DirEntry, root: &Path) -> bool {
    if entry.path() == root {
        return false;
    }
    matches!(
        entry.file_name().to_str(),
        Some(".duke-cas" | ".duke-profile" | ".duke-patch-tmp")
    )
}

fn normalize_rel_path(base: &Path, full: &Path) -> Result<String, String> {
    let rel = full
        .strip_prefix(base)
        .map_err(|e| format!("Failed to relativize path {}: {}", full.display(), e))?;
    let mut parts = Vec::new();
    for comp in rel.components() {
        match comp {
            Component::Normal(seg) => parts.push(seg.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => {
                return Err(format!(
                    "Unsafe path component in delta entry: {}",
                    rel.display()
                ));
            }
        }
    }
    Ok(parts.join("/"))
}

pub fn safe_join_rel_path(base: &Path, rel: &str) -> Result<PathBuf, String> {
    let mut out = base.to_path_buf();
    for seg in rel.split('/') {
        if seg.is_empty()
            || seg == "."
            || seg == ".."
            || seg.contains('\\')
            || seg.contains(':')
            || seg.contains('\0')
        {
            return Err(format!("Unsafe relative path segment in manifest: {rel}"));
        }
        out.push(seg);
    }
    Ok(out)
}

pub fn chunk_single_file(path: &Path) -> Result<(u64, String, Vec<ChunkEntry>), String> {
    let file =
        File::open(path).map_err(|e| format!("Failed to open {}: {}", path.display(), e))?;
    let size_bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut whole_hasher = blake3::Hasher::new();
    let mut chunks = Vec::new();

    if size_bytes > 0 {
        let reader = BufReader::with_capacity(64 * 1024, file);
        let stream = StreamCDC::new(reader, FASTCDC_MIN_SIZE, FASTCDC_AVG_SIZE, FASTCDC_MAX_SIZE);
        for chunk_res in stream {
            let chunk = chunk_res
                .map_err(|e| format!("FastCDC read failed on {}: {}", path.display(), e))?;
            whole_hasher.update(&chunk.data);
            let digest = blake3::hash(&chunk.data).to_hex().to_string();
            chunks.push(ChunkEntry {
                gear_hash: chunk.hash,
                offset: chunk.offset,
                length: chunk.length as u32,
                blake3: digest,
            });
        }
    }

    Ok((size_bytes, whole_hasher.finalize().to_hex().to_string(), chunks))
}

pub fn build_dkmanifest_from_dir(root_dir: &Path, label: &str) -> Result<DkManifest, String> {
    if !root_dir.is_dir() {
        return Err(format!(
            "Target directory does not exist: {}",
            root_dir.display()
        ));
    }

    let mut work = Vec::new();
    for entry in WalkDir::new(root_dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !should_skip_delta_entry(e, root_dir))
    {
        let Ok(entry) = entry else {
            continue;
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = normalize_rel_path(root_dir, entry.path())?;
        if !rel.is_empty() && !rel.ends_with(".dkmanifest") && !rel.ends_with(".dkdelta") {
            work.push((entry.path().to_path_buf(), rel));
        }
    }

    let queue = Arc::new(Mutex::new(work));
    let out_files = Arc::new(Mutex::new(Vec::<ManifestFileEntry>::new()));
    let error_slot = Arc::new(Mutex::new(None::<String>));
    let worker_count = std::thread::available_parallelism()
        .map(|n| n.get().clamp(2, 8))
        .unwrap_or(4);

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let queue = Arc::clone(&queue);
            let out_files = Arc::clone(&out_files);
            let error_slot = Arc::clone(&error_slot);
            scope.spawn(move || loop {
                if error_slot.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
                    break;
                }
                let item = { queue.lock().unwrap_or_else(|e| e.into_inner()).pop() };
                let Some((full_path, rel_path)) = item else {
                    break;
                };
                match chunk_single_file(&full_path) {
                    Ok((size_bytes, blake3, chunks)) => {
                        out_files
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push(ManifestFileEntry {
                                rel_path,
                                size_bytes,
                                blake3,
                                chunks,
                            });
                    }
                    Err(err) => {
                        *error_slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(err);
                        break;
                    }
                }
            });
        }
    });

    if let Some(err) = error_slot.lock().unwrap_or_else(|e| e.into_inner()).take() {
        return Err(err);
    }

    let mut files = std::mem::take(&mut *out_files.lock().unwrap_or_else(|e| e.into_inner()));
    files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    let total_bytes = files.iter().map(|f| f.size_bytes).sum();

    Ok(DkManifest {
        magic: "DKMF001".to_string(),
        root_label: label.to_string(),
        created_at: now_unix_secs(),
        chunk_min: FASTCDC_MIN_SIZE,
        chunk_avg: FASTCDC_AVG_SIZE,
        chunk_max: FASTCDC_MAX_SIZE,
        total_bytes,
        files,
    })
}

pub fn write_dkmanifest_file(manifest: &DkManifest, out_path: &Path) -> Result<(), String> {
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create manifest dir {}: {}", parent.display(), e))?;
    }
    let json_bytes = serde_json::to_vec(manifest)
        .map_err(|e| format!("Failed to serialize .dkmanifest: {}", e))?;
    let compressed = zstd::bulk::compress(&json_bytes, 3)
        .map_err(|e| format!("Failed to compress .dkmanifest with zstd: {}", e))?;

    let tmp_path = out_path.with_extension(format!("dkmanifest.tmp.{}", std::process::id()));
    let mut payload = Vec::with_capacity(DKMF_ZSTD_MAGIC.len() + compressed.len());
    payload.extend_from_slice(DKMF_ZSTD_MAGIC);
    payload.extend_from_slice(&compressed);
    fs::write(&tmp_path, &payload)
        .map_err(|e| format!("Failed to write {}: {}", tmp_path.display(), e))?;
    if out_path.exists() {
        let _ = fs::remove_file(out_path);
    }
    fs::rename(&tmp_path, out_path)
        .map_err(|e| format!("Failed to commit {}: {}", out_path.display(), e))
}

pub fn read_dkmanifest_file(path: &Path) -> Result<DkManifest, String> {
    let raw = fs::read(path)
        .map_err(|e| format!("Failed to read .dkmanifest {}: {}", path.display(), e))?;
    if raw.starts_with(DKMF_ZSTD_MAGIC) {
        let decompressed = zstd::stream::decode_all(&raw[DKMF_ZSTD_MAGIC.len()..])
            .map_err(|e| format!("Failed to decompress .dkmanifest {}: {}", path.display(), e))?;
        serde_json::from_slice::<DkManifest>(&decompressed)
            .map_err(|e| format!("Invalid compressed .dkmanifest {}: {}", path.display(), e))
    } else {
        serde_json::from_slice::<DkManifest>(&raw)
            .map_err(|e| format!("Invalid .dkmanifest {}: {}", path.display(), e))
    }
}

pub fn index_local_chunks(local_dir: &Path) -> Result<HashMap<String, LocalChunkLocation>, String> {
    let mut index = HashMap::new();
    if !local_dir.is_dir() {
        return Ok(index);
    }

    for entry in WalkDir::new(local_dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !should_skip_delta_entry(e, local_dir))
    {
        let Ok(entry) = entry else {
            continue;
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("duke_tmp") {
            continue;
        }
        if let Ok((_, _, chunks)) = chunk_single_file(path) {
            for ch in chunks {
                index.entry(ch.blake3).or_insert_with(|| LocalChunkLocation {
                    file_path: path.to_path_buf(),
                    offset: ch.offset,
                    length: ch.length,
                });
            }
        }
    }
    Ok(index)
}

pub fn diff_local_dir_against_manifest(
    local_dir: &Path,
    target: &DkManifest,
) -> Result<DeltaDiffReport, String> {
    let local_index = index_local_chunks(local_dir)?;
    let mut reused_chunks = 0usize;
    let mut reused_bytes = 0u64;
    let mut missing_chunks = 0usize;
    let mut missing_bytes = 0u64;
    let mut missing_set = HashSet::new();
    let mut missing_hashes = Vec::new();

    for file in &target.files {
        for chunk in &file.chunks {
            if local_index.contains_key(&chunk.blake3) {
                reused_chunks += 1;
                reused_bytes += chunk.length as u64;
            } else {
                missing_chunks += 1;
                missing_bytes += chunk.length as u64;
                if missing_set.insert(chunk.blake3.clone()) {
                    missing_hashes.push(chunk.blake3.clone());
                }
            }
        }
    }

    Ok(DeltaDiffReport {
        target_files: target.files.len(),
        total_target_bytes: target.total_bytes,
        reused_chunks,
        reused_bytes,
        missing_chunks,
        missing_bytes,
        missing_hashes,
    })
}

pub fn create_zstd_delta_bundle(
    source_dir: &Path,
    target_manifest: &DkManifest,
    missing_hashes: &HashSet<String>,
    out_bundle_path: &Path,
) -> Result<DeltaBundleStats, String> {
    let started = Instant::now();
    if let Some(parent) = out_bundle_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create delta dir {}: {}", parent.display(), e))?;
    }

    let mut chunk_locations: HashMap<String, LocalChunkLocation> = HashMap::new();
    for file in &target_manifest.files {
        let full_path = safe_join_rel_path(source_dir, &file.rel_path)?;
        for ch in &file.chunks {
            if missing_hashes.contains(&ch.blake3) && !chunk_locations.contains_key(&ch.blake3) {
                chunk_locations.insert(
                    ch.blake3.clone(),
                    LocalChunkLocation {
                        file_path: full_path.clone(),
                        offset: ch.offset,
                        length: ch.length,
                    },
                );
            }
        }
    }

    for needed in missing_hashes {
        if !chunk_locations.contains_key(needed) {
            return Err(format!(
                "Source directory missing requested chunk hash {}",
                needed
            ));
        }
    }

    let tmp_path = out_bundle_path.with_extension(format!("dkdelta.tmp.{}", std::process::id()));
    let out_file = File::create(&tmp_path)
        .map_err(|e| format!("Failed to create delta bundle {}: {}", tmp_path.display(), e))?;
    let mut encoder = zstd::stream::Encoder::new(BufWriter::new(out_file), 3)
        .map_err(|e| format!("Failed to create zstd delta encoder: {}", e))?;

    encoder
        .write_all(DKDELTA_MAGIC)
        .map_err(|e| format!("Failed writing delta header: {}", e))?;
    let chunk_count = chunk_locations.len() as u64;
    encoder
        .write_all(&chunk_count.to_le_bytes())
        .map_err(|e| format!("Failed writing delta count: {}", e))?;

    let mut uncompressed_bytes = 0u64;
    let mut buf = vec![0u8; FASTCDC_MAX_SIZE as usize];
    for (hash_hex, loc) in &chunk_locations {
        let hash_bytes = hex::decode(hash_hex)
            .map_err(|e| format!("Invalid BLAKE3 hex digest {}: {}", hash_hex, e))?;
        if hash_bytes.len() != 32 {
            return Err(format!("Unexpected BLAKE3 hash length for {}", hash_hex));
        }
        let len = loc.length as usize;
        if len > buf.len() {
            buf.resize(len, 0);
        }
        let mut src = File::open(&loc.file_path)
            .map_err(|e| format!("Failed to open {}: {}", loc.file_path.display(), e))?;
        src.seek(SeekFrom::Start(loc.offset))
            .map_err(|e| format!("Failed to seek {}: {}", loc.file_path.display(), e))?;
        src.read_exact(&mut buf[..len])
            .map_err(|e| format!("Failed to read chunk from {}: {}", loc.file_path.display(), e))?;

        encoder
            .write_all(&hash_bytes)
            .map_err(|e| format!("Failed writing chunk hash: {}", e))?;
        encoder
            .write_all(&loc.length.to_le_bytes())
            .map_err(|e| format!("Failed writing chunk length: {}", e))?;
        encoder
            .write_all(&buf[..len])
            .map_err(|e| format!("Failed writing chunk payload: {}", e))?;
        uncompressed_bytes += len as u64;
    }

    let inner = encoder
        .finish()
        .map_err(|e| format!("Failed finalizing zstd delta stream: {}", e))?;
    inner
        .into_inner()
        .map_err(|e| format!("Failed flushing delta file: {}", e))?
        .sync_all()
        .map_err(|e| format!("Failed syncing delta file: {}", e))?;

    if out_bundle_path.exists() {
        let _ = fs::remove_file(out_bundle_path);
    }
    fs::rename(&tmp_path, out_bundle_path)
        .map_err(|e| format!("Failed finalizing delta bundle: {}", e))?;
    let compressed_bytes = fs::metadata(out_bundle_path).map(|m| m.len()).unwrap_or(0);

    Ok(DeltaBundleStats {
        bundle_path: out_bundle_path.to_string_lossy().into_owned(),
        chunk_count: chunk_locations.len(),
        uncompressed_bytes,
        compressed_bytes,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

fn unpack_delta_bundle_to_spool(
    bundle_path: &Path,
    spool_file_path: &Path,
) -> Result<HashMap<String, (u64, u32)>, String> {
    let f = File::open(bundle_path)
        .map_err(|e| format!("Failed to open delta bundle {}: {}", bundle_path.display(), e))?;
    let mut decoder = zstd::stream::Decoder::new(BufReader::new(f))
        .map_err(|e| format!("Failed to initialize zstd decoder: {}", e))?;

    let mut magic = [0u8; 7];
    decoder
        .read_exact(&mut magic)
        .map_err(|e| format!("Truncated delta header: {}", e))?;
    if &magic != DKDELTA_MAGIC {
        return Err("Invalid .dkdelta header magic".to_string());
    }

    let mut count_buf = [0u8; 8];
    decoder
        .read_exact(&mut count_buf)
        .map_err(|e| format!("Truncated delta chunk count: {}", e))?;
    let count = u64::from_le_bytes(count_buf);

    let mut spool = BufWriter::new(
        File::create(spool_file_path)
            .map_err(|e| format!("Failed creating delta spool: {}", e))?,
    );
    let mut map = HashMap::with_capacity(count as usize);
    let mut offset = 0u64;
    let mut chunk_buf = Vec::new();

    for _ in 0..count {
        let mut hash_raw = [0u8; 32];
        let mut len_buf = [0u8; 4];
        decoder
            .read_exact(&mut hash_raw)
            .map_err(|e| format!("Truncated delta chunk hash: {}", e))?;
        decoder
            .read_exact(&mut len_buf)
            .map_err(|e| format!("Truncated delta chunk length: {}", e))?;
        let len = u32::from_le_bytes(len_buf);
        if len > FASTCDC_MAX_SIZE * 4 {
            return Err(format!("Delta chunk length {} exceeds safety cap", len));
        }
        chunk_buf.resize(len as usize, 0);
        decoder
            .read_exact(&mut chunk_buf)
            .map_err(|e| format!("Truncated delta chunk body: {}", e))?;

        let actual_hash = *blake3::hash(&chunk_buf).as_bytes();
        if actual_hash != hash_raw {
            return Err("Delta chunk failed BLAKE3 integrity check".to_string());
        }
        spool
            .write_all(&chunk_buf)
            .map_err(|e| format!("Failed writing delta spool: {}", e))?;
        map.insert(hex::encode(hash_raw), (offset, len));
        offset += len as u64;
    }

    spool
        .flush()
        .map_err(|e| format!("Failed flushing delta spool: {}", e))?;
    Ok(map)
}

pub fn apply_delta_with_local_stitching(
    local_dir: &Path,
    target_manifest: &DkManifest,
    delta_bundle_path: Option<&Path>,
) -> Result<DeltaApplyReport, String> {
    let started = Instant::now();
    fs::create_dir_all(local_dir)
        .map_err(|e| format!("Failed creating target dir {}: {}", local_dir.display(), e))?;

    let tmp_root = local_dir.join(".duke-patch-tmp");
    let _ = fs::remove_dir_all(&tmp_root);
    fs::create_dir_all(&tmp_root)
        .map_err(|e| format!("Failed creating temp patch dir: {}", e))?;

    let result = (|| -> Result<DeltaApplyReport, String> {
        let local_index = index_local_chunks(local_dir)?;
        let spool_path = tmp_root.join("delta_spool.bin");
        let delta_map = if let Some(bundle) = delta_bundle_path {
            unpack_delta_bundle_to_spool(bundle, &spool_path)?
        } else {
            HashMap::new()
        };
        let mut spool_reader = if !delta_map.is_empty() {
            Some(
                File::open(&spool_path)
                    .map_err(|e| format!("Failed reopening delta spool: {}", e))?,
            )
        } else {
            None
        };

        let mut staged_commits: Vec<(PathBuf, PathBuf)> = Vec::new();
        let mut files_unchanged = 0usize;
        let mut chunks_reused_from_disk = 0usize;
        let mut chunks_applied_from_delta = 0usize;
        let mut bytes_stitched_total = 0u64;
        let mut target_rel_set = HashSet::with_capacity(target_manifest.files.len());
        let mut chunk_buf = vec![0u8; FASTCDC_MAX_SIZE as usize];

        for (file_idx, file) in target_manifest.files.iter().enumerate() {
            target_rel_set.insert(file.rel_path.clone());
            let dest_path = safe_join_rel_path(local_dir, &file.rel_path)?;

            if dest_path.is_file() {
                if let Ok(meta) = fs::metadata(&dest_path) {
                    if meta.len() == file.size_bytes {
                        if let Ok(existing_hash) = crate::snapshots::hash_file_blake3_64k(&dest_path) {
                            if existing_hash == file.blake3 {
                                files_unchanged += 1;
                                continue;
                            }
                        }
                    }
                }
            }

            let staged_path = tmp_root.join(format!("staged_{file_idx}.part"));
            let mut out = BufWriter::new(
                File::create(&staged_path)
                    .map_err(|e| format!("Failed creating staged file: {}", e))?,
            );
            let mut whole_hasher = blake3::Hasher::new();

            for ch in &file.chunks {
                let len = ch.length as usize;
                if len > chunk_buf.len() {
                    chunk_buf.resize(len, 0);
                }

                if let Some((off, dlen)) = delta_map.get(&ch.blake3) {
                    if *dlen != ch.length {
                        return Err(format!("Chunk length mismatch for {}", ch.blake3));
                    }
                    let reader = spool_reader
                        .as_mut()
                        .ok_or_else(|| "Delta spool handle unavailable".to_string())?;
                    reader
                        .seek(SeekFrom::Start(*off))
                        .map_err(|e| format!("Delta spool seek failed: {}", e))?;
                    reader
                        .read_exact(&mut chunk_buf[..len])
                        .map_err(|e| format!("Delta spool read failed: {}", e))?;
                    chunks_applied_from_delta += 1;
                } else if let Some(loc) = local_index.get(&ch.blake3) {
                    if loc.length != ch.length {
                        return Err(format!("Local chunk length mismatch for {}", ch.blake3));
                    }
                    let mut src = File::open(&loc.file_path).map_err(|e| {
                        format!("Failed reading local chunk {}: {}", loc.file_path.display(), e)
                    })?;
                    src.seek(SeekFrom::Start(loc.offset))
                        .map_err(|e| format!("Failed seeking local chunk: {}", e))?;
                    src.read_exact(&mut chunk_buf[..len])
                        .map_err(|e| format!("Failed reading local chunk bytes: {}", e))?;
                    chunks_reused_from_disk += 1;
                } else {
                    return Err(format!(
                        "Missing chunk {} for '{}' (neither on disk nor in delta bundle)",
                        ch.blake3, file.rel_path
                    ));
                }

                let actual_chunk_hex = blake3::hash(&chunk_buf[..len]).to_hex().to_string();
                if actual_chunk_hex != ch.blake3 {
                    return Err(format!(
                        "Chunk verification failed for '{}' at offset {}",
                        file.rel_path, ch.offset
                    ));
                }

                whole_hasher.update(&chunk_buf[..len]);
                out.write_all(&chunk_buf[..len])
                    .map_err(|e| format!("Failed writing staged chunk: {}", e))?;
                bytes_stitched_total += len as u64;
            }

            out.flush()
                .map_err(|e| format!("Failed flushing staged file: {}", e))?;
            drop(out);

            let actual_file_hex = whole_hasher.finalize().to_hex().to_string();
            if actual_file_hex != file.blake3 {
                return Err(format!(
                    "Reconstructed file '{}' failed whole-file BLAKE3 check",
                    file.rel_path
                ));
            }
            staged_commits.push((staged_path, dest_path));
        }

        drop(spool_reader);

        let files_patched = staged_commits.len();
        for (staged, dest) in staged_commits {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed creating parent {}: {}", parent.display(), e))?;
            }
            if dest.exists() {
                let _ = fs::remove_file(&dest);
            }
            fs::rename(&staged, &dest).map_err(|e| {
                format!(
                    "Failed committing stitched file {}: {}",
                    dest.display(),
                    e
                )
            })?;
        }

        let mut files_removed = 0usize;
        for entry in WalkDir::new(local_dir)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !should_skip_delta_entry(e, local_dir))
        {
            let Ok(entry) = entry else {
                continue;
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = normalize_rel_path(local_dir, entry.path())?;
            if rel.ends_with(".dkmanifest") || rel.ends_with(".dkdelta") {
                continue;
            }
            if !target_rel_set.contains(&rel) && fs::remove_file(entry.path()).is_ok() {
                files_removed += 1;
            }
        }

        Ok(DeltaApplyReport {
            files_patched,
            files_unchanged,
            files_removed,
            chunks_reused_from_disk,
            chunks_applied_from_delta,
            bytes_stitched_total,
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    })();

    let _ = fs::remove_dir_all(&tmp_root);
    result
}

fn resolve_game_dir_for_delta(game_id: &str) -> Result<(PathBuf, String), String> {
    let library = load_library_standalone()?;
    let entry = library
        .into_iter()
        .find(|g| g.id == game_id)
        .ok_or_else(|| format!("Game '{}' not found in library.", game_id))?;
    let dir = if !entry.install_dir.trim().is_empty() {
        PathBuf::from(entry.install_dir.trim())
    } else {
        resolve_game_root_dir(Path::new(entry.exe_path.trim()))
    };
    Ok((dir, entry.title))
}

#[tauri::command]
pub async fn generate_game_dkmanifest(
    game_id: String,
    out_path: Option<String>,
) -> Result<DkManifestSummary, String> {
    let (game_dir, title) = resolve_game_dir_for_delta(&game_id)?;
    let target_path = out_path
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| game_dir.join(".duke.dkmanifest"));

    tokio::task::spawn_blocking(move || {
        let started = Instant::now();
        let manifest = build_dkmanifest_from_dir(&game_dir, &title)?;
        write_dkmanifest_file(&manifest, &target_path)?;
        let chunk_count = manifest.files.iter().map(|f| f.chunks.len()).sum();
        Ok(DkManifestSummary {
            manifest_path: target_path.to_string_lossy().into_owned(),
            file_count: manifest.files.len(),
            chunk_count,
            total_bytes: manifest.total_bytes,
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    })
    .await
    .map_err(|e| format!("Manifest generation task failed: {}", e))?
}

#[tauri::command]
pub async fn diff_game_against_manifest(
    game_id: String,
    manifest_path: String,
) -> Result<DeltaDiffReport, String> {
    let (game_dir, _) = resolve_game_dir_for_delta(&game_id)?;
    let mpath = PathBuf::from(manifest_path.trim());
    tokio::task::spawn_blocking(move || {
        let manifest = read_dkmanifest_file(&mpath)?;
        diff_local_dir_against_manifest(&game_dir, &manifest)
    })
    .await
    .map_err(|e| format!("Delta diff task failed: {}", e))?
}

#[tauri::command]
pub async fn create_delta_patch_file(
    source_dir: String,
    manifest_path: String,
    missing_hashes: Vec<String>,
    out_delta_path: String,
) -> Result<DeltaBundleStats, String> {
    let src = PathBuf::from(source_dir.trim());
    let mpath = PathBuf::from(manifest_path.trim());
    let out = PathBuf::from(out_delta_path.trim());
    tokio::task::spawn_blocking(move || {
        let manifest = read_dkmanifest_file(&mpath)?;
        let set: HashSet<String> = missing_hashes.into_iter().collect();
        create_zstd_delta_bundle(&src, &manifest, &set, &out)
    })
    .await
    .map_err(|e| format!("Delta bundle creation task failed: {}", e))?
}

#[tauri::command]
pub async fn apply_game_delta_patch(
    game_id: String,
    manifest_path: String,
    delta_path: Option<String>,
) -> Result<DeltaApplyReport, String> {
    let (game_dir, _) = resolve_game_dir_for_delta(&game_id)?;
    let mpath = PathBuf::from(manifest_path.trim());
    let dpath = delta_path
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    tokio::task::spawn_blocking(move || {
        let manifest = read_dkmanifest_file(&mpath)?;
        apply_delta_with_local_stitching(&game_dir, &manifest, dpath.as_deref())
    })
    .await
    .map_err(|e| format!("Delta apply task failed: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fastcdc_delta_transfers_only_changed_chunks_and_stitches_local_blocks() {
        let base_dir = std::env::temp_dir().join(format!("duke_delta_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base_dir);
        let old_dir = base_dir.join("old_game");
        let new_dir = base_dir.join("new_game");
        fs::create_dir_all(&old_dir).unwrap();
        fs::create_dir_all(&new_dir).unwrap();

        // Build a deterministic 2.5 MiB pseudo-random binary asset so Gear cut points align well.
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut v1 = Vec::with_capacity(2_500_000);
        for _ in 0..2_500_000 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            v1.push((state >> 56) as u8);
        }
        fs::write(old_dir.join("world.pak"), &v1).unwrap();
        fs::write(old_dir.join("readme.txt"), b"v1 notes").unwrap();

        // v2 prepends a 137-byte header and patches one 16 KB window in the middle, shifting offsets!
        let mut v2 = vec![0xABu8; 137];
        v2.extend_from_slice(&v1);
        for byte in &mut v2[1_200_000..1_216_000] {
            *byte ^= 0xFF;
        }
        fs::write(new_dir.join("world.pak"), &v2).unwrap();
        fs::write(new_dir.join("readme.txt"), b"v2 notes!").unwrap();

        let target_manifest = build_dkmanifest_from_dir(&new_dir, "TestGame v2").unwrap();
        let manifest_file = base_dir.join("v2.dkmanifest");
        write_dkmanifest_file(&target_manifest, &manifest_file).unwrap();
        let reloaded = read_dkmanifest_file(&manifest_file).unwrap();

        let diff = diff_local_dir_against_manifest(&old_dir, &reloaded).unwrap();
        assert!(
            diff.reused_bytes > 1_800_000,
            "expected FastCDC to reuse most chunks despite 137-byte shift, reused {}",
            diff.reused_bytes
        );
        assert!(
            diff.missing_bytes < 500_000,
            "expected small delta missing bytes, got {}",
            diff.missing_bytes
        );

        let missing_set: HashSet<String> = diff.missing_hashes.iter().cloned().collect();
        let bundle_path = base_dir.join("patch.dkdelta");
        let stats =
            create_zstd_delta_bundle(&new_dir, &reloaded, &missing_set, &bundle_path).unwrap();
        assert!(stats.compressed_bytes < 500_000);

        let apply_report =
            apply_delta_with_local_stitching(&old_dir, &reloaded, Some(&bundle_path)).unwrap();
        assert_eq!(apply_report.files_patched, 2);
        assert!(apply_report.chunks_reused_from_disk > 20);
        assert_eq!(fs::read(old_dir.join("world.pak")).unwrap(), v2);
        assert_eq!(fs::read(old_dir.join("readme.txt")).unwrap(), b"v2 notes!");

        let _ = fs::remove_dir_all(&base_dir);
    }
}
