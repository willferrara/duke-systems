use crate::protocol::{
    compare_vector_clocks, merge_vector_clocks, ClockOrdering, DkManifest, SwarmAppAd, SwarmBeacon,
    SwarmWireRequest, SwarmWireResponse, VectorClock, DKDELTA_MAGIC, SWARM_MAGIC,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufWriter, Cursor, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_UNPACKED_CHUNKS: u64 = 250_000;
const MAX_CHUNK_BYTES: usize = 4 * 1024 * 1024;
const MAX_SHADER_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct NodePersistentState {
    pub node_id: String,
    pub node_name: String,
    pub save_clocks: BTreeMap<String, VectorClock>,
    pub manifests: BTreeMap<String, ManifestMeta>,
    pub shaders: BTreeMap<String, ShaderMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestMeta {
    pub key: String,
    pub root_label: String,
    pub total_bytes: u64,
    pub file_count: usize,
    pub stored_at_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderMeta {
    pub key: String,
    pub game_blake3: String,
    pub gpu_key: String,
    pub driver_version: String,
    pub file_count: usize,
    pub compressed_bytes: usize,
    pub stored_at_secs: u64,
}

pub struct SwarmStorage {
    root: PathBuf,
    node_name: String,
    tcp_port: u16,
    lock: Mutex<NodePersistentState>,
}

impl SwarmStorage {
    pub fn open(root: impl Into<PathBuf>, node_name: impl Into<String>, tcp_port: u16) -> Result<Arc<Self>, String> {
        let root = root.into();
        let node_name = node_name.into();
        fs::create_dir_all(root.join("cas").join("blobs"))
            .map_err(|e| format!("Failed creating CAS dir: {}", e))?;
        fs::create_dir_all(root.join("saves"))
            .map_err(|e| format!("Failed creating saves dir: {}", e))?;
        fs::create_dir_all(root.join("manifests"))
            .map_err(|e| format!("Failed creating manifests dir: {}", e))?;
        fs::create_dir_all(root.join("shaders"))
            .map_err(|e| format!("Failed creating shaders dir: {}", e))?;

        let state_path = root.join("node_state.json");
        let mut state: NodePersistentState = if state_path.is_file() {
            fs::read_to_string(&state_path)
                .ok()
                .and_then(|raw| serde_json::from_str(&raw).ok())
                .unwrap_or_default()
        } else {
            NodePersistentState::default()
        };

        if state.node_id.trim().is_empty() {
            let seed = format!(
                "{}:{}:{}",
                node_name,
                std::process::id(),
                now_unix_secs()
            );
            state.node_id = blake3::hash(seed.as_bytes()).to_hex().to_string()[..16].to_string();
        }
        state.node_name = node_name.clone();

        let storage = Arc::new(Self {
            root,
            node_name,
            tcp_port,
            lock: Mutex::new(state),
        });
        storage.persist_state()?;
        Ok(storage)
    }

    pub fn node_identity(&self) -> (String, String) {
        let g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        (g.node_id.clone(), self.node_name.clone())
    }

    pub fn snapshot_save_clocks(&self) -> BTreeMap<String, VectorClock> {
        self.lock
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .save_clocks
            .clone()
    }

    pub fn build_beacon(&self) -> SwarmBeacon {
        let guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let apps: Vec<SwarmAppAd> = guard
            .manifests
            .values()
            .map(|m| SwarmAppAd {
                game_key: m.key.clone(),
                local_id: m.key.clone(),
                title: m.root_label.clone(),
                app_id: m.key.parse::<u32>().ok(),
                size_bytes: m.total_bytes,
            })
            .collect();
        SwarmBeacon {
            magic: SWARM_MAGIC.to_string(),
            node_id: guard.node_id.clone(),
            node_name: self.node_name.clone(),
            tcp_port: self.tcp_port,
            apps,
            save_clocks: guard.save_clocks.clone(),
            timestamp_secs: now_unix_secs(),
        }
    }

    pub fn build_save_pull_request(&self, save_key: &str) -> SwarmWireRequest {
        let mut have_hashes = Vec::new();
        if let Ok(Some(manifest)) = self.load_manifest_by_key(&save_manifest_key(save_key)) {
            for f in &manifest.files {
                for c in &f.chunks {
                    if self.has_cas_blob(&c.blake3) {
                        have_hashes.push(c.blake3.clone());
                    }
                }
            }
        }
        SwarmWireRequest::SaveSyncPull {
            save_key: save_key.to_string(),
            have_hashes,
        }
    }

    pub fn build_save_offer_request(&self, save_key: &str) -> Result<Option<SwarmWireRequest>, String> {
        let manifest_opt = self.load_manifest_by_key(&save_manifest_key(save_key))?;
        let Some(manifest) = manifest_opt else {
            return Ok(None);
        };
        let clock = self
            .lock
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .save_clocks
            .get(save_key)
            .cloned()
            .unwrap_or_default();
        let wanted: HashSet<String> = manifest
            .files
            .iter()
            .flat_map(|f| f.chunks.iter().map(|c| c.blake3.clone()))
            .collect();
        let delta_bytes = self.pack_chunks_from_cas_or_save(save_key, &manifest, &wanted)?;
        Ok(Some(SwarmWireRequest::SaveSyncOffer {
            save_key: save_key.to_string(),
            sender_clock: clock,
            manifest: Some(manifest),
            delta_bytes: Some(delta_bytes),
        }))
    }

    pub fn apply_pulled_save(
        &self,
        save_key: &str,
        remote_clock: VectorClock,
        manifest: Option<DkManifest>,
        bundle_bytes: Option<Vec<u8>>,
    ) -> Result<bool, String> {
        self.apply_incoming_save(save_key, remote_clock, manifest, bundle_bytes)
            .map(|(applied, _)| applied)
    }

    pub fn handle_request(&self, req: SwarmWireRequest) -> SwarmWireResponse {
        match req {
            SwarmWireRequest::GetBeacon => SwarmWireResponse::Beacon(self.build_beacon()),
            SwarmWireRequest::BeamManifest { game_key } => {
                match self.load_manifest_by_key(&game_key) {
                    Ok(Some(manifest)) => SwarmWireResponse::Manifest(manifest),
                    Ok(None) => SwarmWireResponse::Error {
                        message: format!("Manifest '{}' not stored on headless node", game_key),
                    },
                    Err(e) => SwarmWireResponse::Error { message: e },
                }
            }
            SwarmWireRequest::BeamDeltaBundle {
                game_key,
                missing_hashes,
            } => {
                let wanted: HashSet<String> = missing_hashes.into_iter().collect();
                match self.build_delta_bundle_for_key(&game_key, &wanted) {
                    Ok(bundle_bytes) => SwarmWireResponse::DeltaBundle { bundle_bytes },
                    Err(e) => SwarmWireResponse::Error { message: e },
                }
            }
            SwarmWireRequest::ManifestOffer {
                manifest_key,
                manifest,
            } => match self.store_manifest(&manifest_key, &manifest) {
                Ok(chunk_count) => SwarmWireResponse::ManifestStored {
                    stored: true,
                    manifest_key,
                    chunk_count,
                },
                Err(e) => SwarmWireResponse::Error { message: e },
            },
            SwarmWireRequest::SaveSyncOffer {
                save_key,
                sender_clock,
                manifest,
                delta_bytes,
            } => match self.apply_incoming_save(&save_key, sender_clock, manifest, delta_bytes) {
                Ok((applied, conflict_backup)) => {
                    let clock = self
                        .lock
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .save_clocks
                        .get(&save_key)
                        .cloned()
                        .unwrap_or_default();
                    SwarmWireResponse::SaveState {
                        clock,
                        manifest: None,
                        bundle_bytes: None,
                        applied,
                        conflict_backup,
                    }
                }
                Err(e) => SwarmWireResponse::Error { message: e },
            },
            SwarmWireRequest::SaveSyncPull {
                save_key,
                have_hashes,
            } => {
                let clock = self
                    .lock
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .save_clocks
                    .get(&save_key)
                    .cloned()
                    .unwrap_or_default();
                let manifest = match self.load_manifest_by_key(&save_manifest_key(&save_key)) {
                    Ok(m) => m,
                    Err(e) => return SwarmWireResponse::Error { message: e },
                };
                if let Some(m) = manifest {
                    let have: HashSet<String> = have_hashes.into_iter().collect();
                    let wanted: HashSet<String> = m
                        .files
                        .iter()
                        .flat_map(|f| f.chunks.iter().map(|c| c.blake3.clone()))
                        .filter(|h| !have.contains(h))
                        .collect();
                    match self.pack_chunks_from_cas_or_save(&save_key, &m, &wanted) {
                        Ok(bundle_bytes) => SwarmWireResponse::SaveState {
                            clock,
                            manifest: Some(m),
                            bundle_bytes: Some(bundle_bytes),
                            applied: false,
                            conflict_backup: false,
                        },
                        Err(e) => SwarmWireResponse::Error { message: e },
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
            } => self.serve_shader_pull(&game_blake3, &gpu_key, &driver_version),
            SwarmWireRequest::ShaderOffer {
                game_blake3,
                gpu_key,
                driver_version,
                file_count,
                bundle_zst_bytes,
            } => match self.store_shader_bundle(
                &game_blake3,
                &gpu_key,
                &driver_version,
                file_count,
                &bundle_zst_bytes,
            ) {
                Ok((key, bytes)) => SwarmWireResponse::ShaderStored {
                    stored: true,
                    key,
                    bytes,
                },
                Err(e) => SwarmWireResponse::Error { message: e },
            },
        }
    }

    pub fn store_manifest(&self, key: &str, manifest: &DkManifest) -> Result<usize, String> {
        let safe_key = sanitize_key(key);
        let path = self.root.join("manifests").join(format!("{}.dkmf.json", safe_key));
        let json = serde_json::to_string_pretty(manifest)
            .map_err(|e| format!("Failed serializing manifest: {}", e))?;
        atomic_write_bytes(&path, json.as_bytes())?;

        let chunk_count: usize = manifest.files.iter().map(|f| f.chunks.len()).sum();
        {
            let mut guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
            guard.manifests.insert(
                safe_key.clone(),
                ManifestMeta {
                    key: key.to_string(),
                    root_label: manifest.root_label.clone(),
                    total_bytes: manifest.total_bytes,
                    file_count: manifest.files.len(),
                    stored_at_secs: now_unix_secs(),
                },
            );
        }
        self.persist_state()?;
        Ok(chunk_count)
    }

    pub fn load_manifest_by_key(&self, key: &str) -> Result<Option<DkManifest>, String> {
        let safe_key = sanitize_key(key);
        let path = self.root.join("manifests").join(format!("{}.dkmf.json", safe_key));
        if !path.is_file() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&path)
            .map_err(|e| format!("Failed reading manifest {}: {}", path.display(), e))?;
        let parsed = serde_json::from_str::<DkManifest>(&raw)
            .map_err(|e| format!("Corrupt manifest {}: {}", path.display(), e))?;
        Ok(Some(parsed))
    }

    fn apply_incoming_save(
        &self,
        save_key: &str,
        sender_clock: VectorClock,
        manifest: Option<DkManifest>,
        delta_bytes: Option<Vec<u8>>,
    ) -> Result<(bool, bool), String> {
        let local_clock = self
            .lock
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .save_clocks
            .get(save_key)
            .cloned()
            .unwrap_or_default();
        let ord = compare_vector_clocks(&local_clock, &sender_clock);
        if !matches!(ord, ClockOrdering::Before | ClockOrdering::Concurrent) {
            return Ok((false, false));
        }

        let safe_save = sanitize_key(save_key);
        let save_dir = self.root.join("saves").join(&safe_save);
        let mut conflict_backup = false;
        if matches!(ord, ClockOrdering::Concurrent) && save_dir.is_dir() {
            let backup_dir = self
                .root
                .join("saves")
                .join("_conflicts")
                .join(format!("{}_{}", safe_save, now_unix_secs()));
            copy_dir_recursive(&save_dir, &backup_dir)?;
            conflict_backup = true;
        }

        let mut applied = false;
        if let Some(bytes) = delta_bytes.as_deref().filter(|b| !b.is_empty()) {
            self.ingest_dkdelta_into_cas(bytes)?;
        }
        if let Some(ref target_manifest) = manifest {
            fs::create_dir_all(&save_dir)
                .map_err(|e| format!("Failed creating save dir {}: {}", save_dir.display(), e))?;
            self.materialize_manifest_from_cas(&save_dir, target_manifest)?;
            self.store_manifest(&save_manifest_key(save_key), target_manifest)?;
            applied = true;
        }

        {
            let mut guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
            let slot = guard.save_clocks.entry(save_key.to_string()).or_default();
            merge_vector_clocks(slot, &sender_clock);
        }
        self.persist_state()?;
        Ok((applied, conflict_backup))
    }

    pub fn ingest_dkdelta_into_cas(&self, bundle_bytes: &[u8]) -> Result<usize, String> {
        let mut decoder = zstd::stream::Decoder::new(Cursor::new(bundle_bytes))
            .map_err(|e| format!("Invalid zstd stream in .dkdelta: {}", e))?;
        let mut magic = [0u8; 7];
        decoder
            .read_exact(&mut magic)
            .map_err(|e| format!("Truncated .dkdelta magic: {}", e))?;
        if &magic != DKDELTA_MAGIC {
            return Err("Invalid DKDLT01 magic header".to_string());
        }
        let mut count_buf = [0u8; 8];
        decoder
            .read_exact(&mut count_buf)
            .map_err(|e| format!("Truncated .dkdelta chunk count: {}", e))?;
        let count = u64::from_le_bytes(count_buf);
        if count > MAX_UNPACKED_CHUNKS {
            return Err("Delta bundle chunk count exceeds safety bound".to_string());
        }

        let mut stored = 0usize;
        for _ in 0..count {
            let mut hash_raw = [0u8; 32];
            decoder
                .read_exact(&mut hash_raw)
                .map_err(|e| format!("Truncated chunk digest: {}", e))?;
            let mut len_raw = [0u8; 4];
            decoder
                .read_exact(&mut len_raw)
                .map_err(|e| format!("Truncated chunk length: {}", e))?;
            let len = u32::from_le_bytes(len_raw) as usize;
            if len > MAX_CHUNK_BYTES {
                return Err(format!("Chunk size {} exceeds 4 MiB ceiling", len));
            }
            let mut data = vec![0u8; len];
            decoder
                .read_exact(&mut data)
                .map_err(|e| format!("Truncated chunk payload: {}", e))?;

            let actual = blake3::hash(&data);
            if actual.as_bytes() != &hash_raw {
                return Err("Chunk BLAKE3 verification failed inside delta stream".to_string());
            }
            let hex_digest = actual.to_hex().to_string();
            let blob_path = self.cas_blob_path(&hex_digest)?;
            if !blob_path.exists() {
                atomic_write_bytes(&blob_path, &data)?;
            }
            stored += 1;
        }
        Ok(stored)
    }

    fn materialize_manifest_from_cas(
        &self,
        target_dir: &Path,
        manifest: &DkManifest,
    ) -> Result<(), String> {
        let mut expected_paths = HashSet::new();
        for f in &manifest.files {
            expected_paths.insert(f.rel_path.clone());
            let dest = safe_join_rel_path(target_dir, &f.rel_path)?;
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed creating parent dir: {}", e))?;
            }
            let tmp = dest.with_extension("duke_node_tmp");
            {
                let out = File::create(&tmp)
                    .map_err(|e| format!("Failed creating temp save {}: {}", tmp.display(), e))?;
                let mut writer = BufWriter::new(out);
                let mut hasher = blake3::Hasher::new();
                for ch in &f.chunks {
                    let data = self.read_cas_or_local_chunk(target_dir, &ch.blake3)?;
                    hasher.update(&data);
                    writer
                        .write_all(&data)
                        .map_err(|e| format!("Failed writing chunk to {}: {}", tmp.display(), e))?;
                }
                writer
                    .flush()
                    .map_err(|e| format!("Failed flushing {}: {}", tmp.display(), e))?;
                let final_hex = hasher.finalize().to_hex().to_string();
                if !f.blake3.is_empty() && final_hex != f.blake3 {
                    let _ = fs::remove_file(&tmp);
                    return Err(format!(
                        "Save integrity mismatch for {} (expected {}, got {})",
                        f.rel_path, f.blake3, final_hex
                    ));
                }
            }
            if dest.exists() {
                let _ = fs::remove_file(&dest);
            }
            fs::rename(&tmp, &dest)
                .map_err(|e| format!("Failed finalizing {}: {}", dest.display(), e))?;
        }
        Ok(())
    }

    fn read_cas_or_local_chunk(&self, local_dir: &Path, hash_hex: &str) -> Result<Vec<u8>, String> {
        let cas = self.cas_blob_path(hash_hex)?;
        if cas.is_file() {
            return fs::read(&cas)
                .map_err(|e| format!("Failed reading CAS blob {}: {}", cas.display(), e));
        }
        for entry in walkdir::WalkDir::new(local_dir)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            if !entry.file_type().is_file() {
                continue;
            }
            if let Ok(chunks) = chunk_file_locations(entry.path()) {
                for (offset, length, hex) in chunks {
                    if hex == hash_hex {
                        let mut f = File::open(entry.path())
                            .map_err(|e| format!("Open failed: {}", e))?;
                        f.seek(SeekFrom::Start(offset))
                            .map_err(|e| format!("Seek failed: {}", e))?;
                        let mut buf = vec![0u8; length as usize];
                        f.read_exact(&mut buf)
                            .map_err(|e| format!("Read failed: {}", e))?;
                        return Ok(buf);
                    }
                }
            }
        }
        Err(format!("Missing BLAKE3 chunk {} in CAS and local saves", hash_hex))
    }

    fn pack_chunks_from_cas_or_save(
        &self,
        save_key: &str,
        manifest: &DkManifest,
        wanted: &HashSet<String>,
    ) -> Result<Vec<u8>, String> {
        let save_dir = self.root.join("saves").join(sanitize_key(save_key));
        let mut payloads: HashMap<String, Vec<u8>> = HashMap::new();
        for f in &manifest.files {
            for ch in &f.chunks {
                if !wanted.contains(&ch.blake3) || payloads.contains_key(&ch.blake3) {
                    continue;
                }
                let data = if let Some(blob) = self.cas_blob_path(&ch.blake3).ok().filter(|p| p.is_file()) {
                    fs::read(&blob).map_err(|e| format!("Read CAS blob error: {}", e))?
                } else {
                    let src_path = safe_join_rel_path(&save_dir, &f.rel_path)?;
                    let mut src = File::open(&src_path)
                        .map_err(|e| format!("Missing chunk source {}: {}", src_path.display(), e))?;
                    src.seek(SeekFrom::Start(ch.offset))
                        .map_err(|e| format!("Seek failed on {}: {}", src_path.display(), e))?;
                    let mut buf = vec![0u8; ch.length as usize];
                    src.read_exact(&mut buf)
                        .map_err(|e| format!("Read failed on {}: {}", src_path.display(), e))?;
                    buf
                };
                payloads.insert(ch.blake3.clone(), data);
            }
        }
        encode_dkdelta_bytes(&payloads)
    }

    fn build_delta_bundle_for_key(
        &self,
        key: &str,
        wanted: &HashSet<String>,
    ) -> Result<Vec<u8>, String> {
        let Some(manifest) = self.load_manifest_by_key(key)? else {
            return Err(format!("Manifest '{}' not found on node", key));
        };
        let mut payloads: HashMap<String, Vec<u8>> = HashMap::new();
        for f in &manifest.files {
            for ch in &f.chunks {
                if wanted.contains(&ch.blake3) && !payloads.contains_key(&ch.blake3) {
                    let blob = self.cas_blob_path(&ch.blake3)?;
                    let data = fs::read(&blob).map_err(|e| {
                        format!("Missing CAS blob {} for manifest '{}': {}", ch.blake3, key, e)
                    })?;
                    payloads.insert(ch.blake3.clone(), data);
                }
            }
        }
        encode_dkdelta_bytes(&payloads)
    }

    pub fn store_shader_bundle(
        &self,
        game_blake3: &str,
        gpu_key: &str,
        driver_version: &str,
        file_count: usize,
        bundle_zst_bytes: &[u8],
    ) -> Result<(String, usize), String> {
        if bundle_zst_bytes.is_empty() || bundle_zst_bytes.len() > MAX_SHADER_BYTES {
            return Err("Invalid shader bundle payload size".to_string());
        }
        let key = shader_cache_key(game_blake3, gpu_key, driver_version);
        let safe = sanitize_key(&key);
        let out_path = self.root.join("shaders").join(format!("{}.dkshd.zst", safe));
        atomic_write_bytes(&out_path, bundle_zst_bytes)?;
        {
            let mut guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
            guard.shaders.insert(
                key.clone(),
                ShaderMeta {
                    key: key.clone(),
                    game_blake3: game_blake3.to_string(),
                    gpu_key: gpu_key.to_string(),
                    driver_version: driver_version.to_string(),
                    file_count,
                    compressed_bytes: bundle_zst_bytes.len(),
                    stored_at_secs: now_unix_secs(),
                },
            );
        }
        self.persist_state()?;
        Ok((key, bundle_zst_bytes.len()))
    }

    fn serve_shader_pull(
        &self,
        game_blake3: &str,
        gpu_key: &str,
        driver_version: &str,
    ) -> SwarmWireResponse {
        let key = shader_cache_key(game_blake3, gpu_key, driver_version);
        let meta_opt = self
            .lock
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .shaders
            .get(&key)
            .cloned();
        let path = self
            .root
            .join("shaders")
            .join(format!("{}.dkshd.zst", sanitize_key(&key)));
        if path.is_file() {
            if let Ok(bytes) = fs::read(&path) {
                return SwarmWireResponse::ShaderBundle {
                    found: true,
                    bundle_zst_bytes: bytes,
                    key,
                    file_count: meta_opt.map(|m| m.file_count).unwrap_or(0),
                };
            }
        }
        SwarmWireResponse::ShaderBundle {
            found: false,
            bundle_zst_bytes: Vec::new(),
            key,
            file_count: 0,
        }
    }

    fn has_cas_blob(&self, hex_digest: &str) -> bool {
        self.cas_blob_path(hex_digest)
            .map(|p| p.is_file())
            .unwrap_or(false)
    }

    fn cas_blob_path(&self, hex_digest: &str) -> Result<PathBuf, String> {
        let trimmed = hex_digest.trim().to_ascii_lowercase();
        if trimmed.len() != 64 || !trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("Invalid BLAKE3 hex digest '{}'", hex_digest));
        }
        Ok(self.root.join("cas").join("blobs").join(&trimmed[..2]).join(trimmed))
    }

    fn persist_state(&self) -> Result<(), String> {
        let snapshot = self.lock.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let json = serde_json::to_string_pretty(&snapshot)
            .map_err(|e| format!("Failed serializing node state: {}", e))?;
        atomic_write_bytes(&self.root.join("node_state.json"), json.as_bytes())
    }
}

pub fn encode_dkdelta_bytes(chunks: &HashMap<String, Vec<u8>>) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut encoder = zstd::stream::Encoder::new(&mut out, 3)
            .map_err(|e| format!("Failed creating zstd delta encoder: {}", e))?;
        encoder
            .write_all(DKDELTA_MAGIC)
            .map_err(|e| format!("Failed writing delta header: {}", e))?;
        encoder
            .write_all(&(chunks.len() as u64).to_le_bytes())
            .map_err(|e| format!("Failed writing delta count: {}", e))?;
        for (hash_hex, data) in chunks {
            let hash_bytes = hex::decode(hash_hex)
                .map_err(|e| format!("Invalid BLAKE3 hex digest {}: {}", hash_hex, e))?;
            if hash_bytes.len() != 32 {
                return Err(format!("Unexpected digest byte length for {}", hash_hex));
            }
            encoder
                .write_all(&hash_bytes)
                .map_err(|e| format!("Failed writing hash: {}", e))?;
            encoder
                .write_all(&(data.len() as u32).to_le_bytes())
                .map_err(|e| format!("Failed writing len: {}", e))?;
            encoder
                .write_all(data)
                .map_err(|e| format!("Failed writing data: {}", e))?;
        }
        encoder
            .finish()
            .map_err(|e| format!("Failed finishing zstd stream: {}", e))?;
    }
    Ok(out)
}

fn chunk_file_locations(path: &Path) -> Result<Vec<(u64, u32, String)>, String> {
    let f = File::open(path).map_err(|e| format!("Open failed: {}", e))?;
    let streamer = fastcdc::v2020::StreamCDC::new(f, 16 * 1024, 32 * 1024, 64 * 1024);
    let mut out = Vec::new();
    for res in streamer {
        let ch = res.map_err(|e| format!("FastCDC error: {}", e))?;
        let digest = blake3::hash(&ch.data).to_hex().to_string();
        out.push((ch.offset, ch.length as u32, digest));
    }
    Ok(out)
}

pub fn save_manifest_key(save_key: &str) -> String {
    format!("save_{}", save_key)
}

fn shader_cache_key(game_blake3: &str, gpu_key: &str, driver_version: &str) -> String {
    format!("{}|{}|{}", game_blake3, gpu_key, driver_version)
}

fn sanitize_key(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.trim().is_empty() {
        "unnamed".to_string()
    } else {
        cleaned.chars().take(120).collect()
    }
}

fn safe_join_rel_path(base: &Path, rel: &str) -> Result<PathBuf, String> {
    let normalized = rel.replace('\\', "/");
    let mut dest = base.to_path_buf();
    for comp in Path::new(&normalized).components() {
        match comp {
            Component::Normal(p) => dest.push(p),
            Component::CurDir => {}
            _ => return Err(format!("Unsafe relative path rejected: '{}'", rel)),
        }
    }
    Ok(dest)
}

fn atomic_write_bytes(dest: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed creating directory {}: {}", parent.display(), e))?;
    }
    let tmp = dest.with_extension(format!("tmp.{}", std::process::id()));
    {
        let mut f = File::create(&tmp)
            .map_err(|e| format!("Failed creating temp file {}: {}", tmp.display(), e))?;
        f.write_all(bytes)
            .map_err(|e| format!("Failed writing temp file {}: {}", tmp.display(), e))?;
        f.sync_all()
            .map_err(|e| format!("Failed syncing {}: {}", tmp.display(), e))?;
    }
    if dest.exists() {
        let _ = fs::remove_file(dest);
    }
    fs::rename(&tmp, dest).map_err(|e| format!("Failed renaming to {}: {}", dest.display(), e))
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("Failed creating {}: {}", dst.display(), e))?;
    for entry in walkdir::WalkDir::new(src)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        let rel = match entry.path().strip_prefix(src) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            let _ = fs::create_dir_all(&target);
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::copy(entry.path(), &target);
        }
    }
    Ok(())
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
