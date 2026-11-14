use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

use crate::{atomic_write_string, load_library_standalone, resolve_game_root_dir};

const HASH_CHUNK_BYTES: usize = 64 * 1024;
const HARDLINK_MIN_BYTES: u64 = 1024 * 1024;
pub(crate) const CAS_DIR_NAME: &str = ".duke-cas";
const PROFILE_DIR_NAME: &str = ".duke-profile";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotFileRecord {
    pub rel_path: String,
    pub size_bytes: u64,
    pub mtime_ns: u64,
    pub blake3: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSnapshotManifest {
    pub id: String,
    pub game_id: String,
    pub label: String,
    pub created_at: u64,
    pub file_count: usize,
    pub total_bytes: u64,
    pub unique_blobs_added: usize,
    pub deduplicated_files: usize,
    pub elapsed_ms: u64,
    pub files: Vec<SnapshotFileRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSnapshotSummary {
    pub id: String,
    pub game_id: String,
    pub label: String,
    pub created_at: u64,
    pub file_count: usize,
    pub total_bytes: u64,
    pub unique_blobs_added: usize,
    pub deduplicated_files: usize,
    pub elapsed_ms: u64,
}

impl From<&GameSnapshotManifest> for GameSnapshotSummary {
    fn from(m: &GameSnapshotManifest) -> Self {
        Self {
            id: m.id.clone(),
            game_id: m.game_id.clone(),
            label: m.label.clone(),
            created_at: m.created_at,
            file_count: m.file_count,
            total_bytes: m.total_bytes,
            unique_blobs_added: m.unique_blobs_added,
            deduplicated_files: m.deduplicated_files,
            elapsed_ms: m.elapsed_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotRestoreReport {
    pub snapshot_id: String,
    pub label: String,
    pub restored_files: usize,
    pub unchanged_files: usize,
    pub hardlinked_files: usize,
    pub copied_files: usize,
    pub removed_files: usize,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotGcReport {
    pub games_scanned: usize,
    pub snapshots_pruned: usize,
    pub blobs_removed: usize,
    pub bytes_freed: u64,
    pub freed_formatted: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct StatIndexCache {
    files: HashMap<String, (u64, u64, String)>,
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

static SNAPSHOT_SEQ: AtomicU64 = AtomicU64::new(1);

fn now_unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn next_snapshot_id() -> String {
    let seq = SNAPSHOT_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("snap-{}-{seq}", now_unix_millis())
}

fn file_mtime_ns(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
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
                    "Unsafe path component in snapshot entry: {}",
                    rel.display()
                ));
            }
        }
    }
    Ok(parts.join("/"))
}

fn safe_join_rel(base: &Path, rel: &str) -> Result<PathBuf, String> {
    let mut out = base.to_path_buf();
    for seg in rel.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." || seg.contains('\\') || seg.contains(':') {
            return Err(format!("Unsafe relative path segment in manifest: {rel}"));
        }
        out.push(seg);
    }
    Ok(out)
}

fn blob_path_for_hash(cas_dir: &Path, hash: &str) -> PathBuf {
    let prefix = hash.get(..2).unwrap_or("00");
    cas_dir
        .join("blobs")
        .join(prefix)
        .join(format!("{hash}.blob"))
}

fn should_skip_walk_entry(entry: &walkdir::DirEntry, root: &Path) -> bool {
    if entry.path() == root {
        return false;
    }
    matches!(
        entry.file_name().to_str(),
        Some(CAS_DIR_NAME | PROFILE_DIR_NAME)
    )
}

pub fn hash_file_blake3_64k(path: &Path) -> Result<String, String> {
    let mut file =
        File::open(path).map_err(|e| format!("Failed to open {}: {}", path.display(), e))?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; HASH_CHUNK_BYTES];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

static BLOB_TMP_SEQ: AtomicU64 = AtomicU64::new(1);

fn ensure_blob_stored(cas_dir: &Path, hash: &str, src_path: &Path, size_bytes: u64) -> Result<bool, String> {
    let blob_path = blob_path_for_hash(cas_dir, hash);
    if blob_path.exists() {
        return Ok(false);
    }
    if let Some(parent) = blob_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create CAS bucket {}: {}", parent.display(), e))?;
    }

    let pid = std::process::id();
    let seq = BLOB_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_path = blob_path.with_extension(format!("tmp.{pid}.{seq}"));

    // Large immutable blobs (>= 1 MiB) use a zero-space NTFS hard link via Win32 CreateHardLinkW
    // when on the same volume, while smaller files are copied so in-place O_TRUNC edits to
    // config/save files never mutate immutable CAS blobs.
    if size_bytes >= HARDLINK_MIN_BYTES {
        match duke_core::win32_ffi::create_hard_link(&tmp_path, src_path) {
            Ok(()) => {
                if let Err(err) = fs::rename(&tmp_path, &blob_path) {
                    let _ = fs::remove_file(&tmp_path);
                    if blob_path.exists() {
                        return Ok(false);
                    }
                    return Err(format!(
                        "Failed to finalize CAS blob {}: {}",
                        blob_path.display(),
                        err
                    ));
                }
                return Ok(true);
            }
            Err(_) => {
                let _ = fs::remove_file(&tmp_path);
            }
        }
    }

    fs::copy(src_path, &tmp_path).map_err(|e| {
        format!(
            "Failed to stage CAS blob from {}: {}",
            src_path.display(),
            e
        )
    })?;
    if let Err(err) = fs::rename(&tmp_path, &blob_path) {
        let _ = fs::remove_file(&tmp_path);
        if blob_path.exists() {
            return Ok(false);
        }
        return Err(format!(
            "Failed to commit CAS blob {}: {}",
            blob_path.display(),
            err
        ));
    }
    Ok(true)
}

#[allow(clippy::permissions_set_readonly_false)]
fn clear_readonly_if_present(path: &Path) {
    if let Ok(meta) = fs::metadata(path) {
        let mut perms = meta.permissions();
        // Windows: clearing `readonly` only toggles `FILE_ATTRIBUTE_READONLY`, never Unix mode bits.
        if perms.readonly() {
            perms.set_readonly(false);
            let _ = fs::set_permissions(path, perms);
        }
    }
}

fn link_or_copy_from_blob(
    target_path: &Path,
    blob_path: &Path,
    size_bytes: u64,
) -> Result<bool, String> {
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create target folder {}: {}", parent.display(), e))?;
    }
    if target_path.exists() {
        clear_readonly_if_present(target_path);
        fs::remove_file(target_path)
            .map_err(|e| format!("Failed to replace {}: {}", target_path.display(), e))?;
    }

    if size_bytes >= HARDLINK_MIN_BYTES {
        match duke_core::win32_ffi::create_hard_link(target_path, blob_path) {
            Ok(()) => return Ok(true),
            Err(_) => {
                // Cross-drive CAS fallback: copy blob bytes when NTFS hardlink spans volumes.
            }
        }
    }

    fs::copy(blob_path, target_path).map_err(|e| {
        format!(
            "Failed to restore {} from CAS blob {}: {}",
            target_path.display(),
            blob_path.display(),
            e
        )
    })?;
    Ok(false)
}

fn load_stat_index(cas_dir: &Path) -> StatIndexCache {
    let idx_path = cas_dir.join("stat_index.json");
    fs::read_to_string(idx_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<StatIndexCache>(&raw).ok())
        .unwrap_or_default()
}

fn save_stat_index(cas_dir: &Path, records: &[SnapshotFileRecord]) {
    let mut cache = StatIndexCache::default();
    for rec in records {
        cache.files.insert(
            rec.rel_path.clone(),
            (rec.size_bytes, rec.mtime_ns, rec.blake3.clone()),
        );
    }
    if let Ok(serialized) = serde_json::to_string(&cache) {
        let _ = atomic_write_string(&cas_dir.join("stat_index.json"), &serialized);
    }
}

fn load_manifest(manifest_path: &Path) -> Result<GameSnapshotManifest, String> {
    let raw = fs::read_to_string(manifest_path)
        .map_err(|e| format!("Failed to read manifest {}: {}", manifest_path.display(), e))?;
    serde_json::from_str::<GameSnapshotManifest>(&raw)
        .map_err(|e| format!("Invalid snapshot manifest {}: {}", manifest_path.display(), e))
}

fn resolve_game_dir_by_id(game_id: &str) -> Result<PathBuf, String> {
    let (dir, _) = resolve_snapshot_scope_by_id(game_id)?;
    Ok(dir)
}

fn rel_matches_subdirs(rel_path: &str, include_subdirs: Option<&[&str]>) -> bool {
    let Some(subs) = include_subdirs else {
        return true;
    };
    let rel_norm = rel_path.trim_matches('/').to_ascii_lowercase();
    subs.iter().any(|s| {
        let s_norm = s.trim_matches(['/', '\\']).to_ascii_lowercase();
        rel_norm == s_norm || rel_norm.starts_with(&format!("{}/", s_norm))
    })
}

fn resolve_snapshot_scope_by_id(game_id: &str) -> Result<(PathBuf, Option<Vec<&'static str>>), String> {
    let library = load_library_standalone()?;
    let entry = library
        .into_iter()
        .find(|g| g.id == game_id)
        .ok_or_else(|| format!("Game '{}' not found in library.", game_id))?;

    if entry.platform_id > 0 {
        if let Some((emu_base, save_subdirs, _)) =
            crate::emulators::resolve_console_save_dirs_for_game(entry.platform_id, &entry.emulator_id)
        {
            fs::create_dir_all(&emu_base)
                .map_err(|e| format!("Failed creating console save root {}: {}", emu_base.display(), e))?;
            for sub in &save_subdirs {
                let _ = fs::create_dir_all(emu_base.join(sub));
            }
            return Ok((emu_base, Some(save_subdirs)));
        }
    }

    let candidate = if !entry.install_dir.trim().is_empty() {
        PathBuf::from(entry.install_dir.trim())
    } else {
        resolve_game_root_dir(Path::new(entry.exe_path.trim()))
    };
    if !candidate.is_dir() {
        return Err(format!(
            "Game directory does not exist: {}",
            candidate.display()
        ));
    }
    Ok((candidate, None))
}

pub fn create_snapshot_in_dir(
    game_id: &str,
    game_dir: &Path,
    cas_dir: &Path,
    label: Option<&str>,
) -> Result<GameSnapshotManifest, String> {
    create_snapshot_in_dir_filtered(game_id, game_dir, cas_dir, label, None)
}

pub fn create_snapshot_in_dir_filtered(
    game_id: &str,
    game_dir: &Path,
    cas_dir: &Path,
    label: Option<&str>,
    include_subdirs: Option<&[&str]>,
) -> Result<GameSnapshotManifest, String> {
    if !game_dir.is_dir() {
        return Err(format!(
            "Target snapshot directory does not exist: {}",
            game_dir.display()
        ));
    }
    let started = Instant::now();
    fs::create_dir_all(cas_dir.join("blobs"))
        .map_err(|e| format!("Failed to init CAS blobs folder: {}", e))?;
    fs::create_dir_all(cas_dir.join("manifests"))
        .map_err(|e| format!("Failed to init CAS manifests folder: {}", e))?;

    let prev_index = Arc::new(load_stat_index(cas_dir));

    let mut work_items: Vec<(PathBuf, String, u64, u64)> = Vec::new();
    for entry in WalkDir::new(game_dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !should_skip_walk_entry(e, game_dir))
    {
        let entry = match entry {
            Ok(v) => v,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let rel_path = normalize_rel_path(game_dir, entry.path())?;
        if rel_path.is_empty() || !rel_matches_subdirs(&rel_path, include_subdirs) {
            continue;
        }
        work_items.push((
            entry.path().to_path_buf(),
            rel_path,
            meta.len(),
            file_mtime_ns(&meta),
        ));
    }

    let total_files = work_items.len();
    let queue = Arc::new(Mutex::new(work_items));
    let results = Arc::new(Mutex::new(Vec::<SnapshotFileRecord>::with_capacity(
        total_files,
    )));
    let error_slot = Arc::new(Mutex::new(None::<String>));
    let blobs_added = Arc::new(Mutex::new(0usize));
    let dedup_count = Arc::new(Mutex::new(0usize));
    let hash_locks = Arc::new(Mutex::new(HashMap::<String, Arc<Mutex<()>>>::new()));

    let worker_count = std::thread::available_parallelism()
        .map(|n| n.get().clamp(2, 8))
        .unwrap_or(4);

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let queue = Arc::clone(&queue);
            let results = Arc::clone(&results);
            let error_slot = Arc::clone(&error_slot);
            let prev_index = Arc::clone(&prev_index);
            let blobs_added = Arc::clone(&blobs_added);
            let dedup_count = Arc::clone(&dedup_count);
            let hash_locks = Arc::clone(&hash_locks);

            scope.spawn(move || {
                let mut local_buf = vec![0u8; HASH_CHUNK_BYTES];
                loop {
                    if error_slot.lock().unwrap().is_some() {
                        break;
                    }
                    let item = {
                        let mut guard = queue.lock().unwrap();
                        guard.pop()
                    };
                    let Some((full_path, rel_path, size_bytes, mtime_ns)) = item else {
                        break;
                    };

                    // Fast path: if (size, mtime_ns) matches our previous snapshot AND its blob is
                    // still intact in .duke-cas, reuse its BLAKE3 digest in O(1) without reading SSD!
                    let cached_hash = prev_index.files.get(&rel_path).and_then(|(s, m, h)| {
                        if *s == size_bytes && *m == mtime_ns && blob_path_for_hash(cas_dir, h).exists() {
                            Some(h.clone())
                        } else {
                            None
                        }
                    });

                    let blake3_hex = match cached_hash {
                        Some(h) => h,
                        None => {
                            let mut file = match File::open(&full_path) {
                                Ok(f) => f,
                                Err(e) => {
                                    *error_slot.lock().unwrap() =
                                        Some(format!("Failed to open {}: {}", full_path.display(), e));
                                    break;
                                }
                            };
                            let mut hasher = blake3::Hasher::new();
                            let mut read_err = None;
                            loop {
                                match file.read(&mut local_buf) {
                                    Ok(0) => break,
                                    Ok(n) => {
                                        hasher.update(&local_buf[..n]);
                                    }
                                    Err(e) => {
                                        read_err = Some(format!(
                                            "Failed to read {}: {}",
                                            full_path.display(),
                                            e
                                        ));
                                        break;
                                    }
                                }
                            }
                            if let Some(err) = read_err {
                                *error_slot.lock().unwrap() = Some(err);
                                break;
                            }
                            hasher.finalize().to_hex().to_string()
                        }
                    };

                    let per_hash_lock = {
                        let mut locks = hash_locks.lock().unwrap();
                        Arc::clone(
                            locks
                                .entry(blake3_hex.clone())
                                .or_insert_with(|| Arc::new(Mutex::new(()))),
                        )
                    };
                    let _hash_guard = per_hash_lock.lock().unwrap();
                    match ensure_blob_stored(cas_dir, &blake3_hex, &full_path, size_bytes) {
                        Ok(true) => {
                            *blobs_added.lock().unwrap() += 1;
                        }
                        Ok(false) => {
                            *dedup_count.lock().unwrap() += 1;
                        }
                        Err(err) => {
                            *error_slot.lock().unwrap() = Some(err);
                            break;
                        }
                    }

                    results.lock().unwrap().push(SnapshotFileRecord {
                        rel_path,
                        size_bytes,
                        mtime_ns,
                        blake3: blake3_hex,
                    });
                }
            });
        }
    });

    if let Some(err) = error_slot.lock().unwrap().take() {
        return Err(err);
    }

    let mut files = std::mem::take(&mut *results.lock().unwrap());
    files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    let total_bytes = files.iter().map(|f| f.size_bytes).sum();
    save_stat_index(cas_dir, &files);

    let created_at = now_unix_secs();
    let id = next_snapshot_id();
    let clean_label = label
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("Snapshot {}", created_at));

    let manifest = GameSnapshotManifest {
        id: id.clone(),
        game_id: game_id.to_string(),
        label: clean_label,
        created_at,
        file_count: files.len(),
        total_bytes,
        unique_blobs_added: *blobs_added.lock().unwrap(),
        deduplicated_files: *dedup_count.lock().unwrap(),
        elapsed_ms: started.elapsed().as_millis() as u64,
        files,
    };

    let manifest_path = cas_dir.join("manifests").join(format!("{}.json", id));
    let serialized = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("Failed to serialize snapshot manifest: {}", e))?;
    atomic_write_string(&manifest_path, &serialized)?;
    Ok(manifest)
}

pub fn list_snapshots_in_cas(cas_dir: &Path) -> Result<Vec<GameSnapshotSummary>, String> {
    let manifests_dir = cas_dir.join("manifests");
    if !manifests_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    let entries = fs::read_dir(&manifests_dir)
        .map_err(|e| format!("Failed to read manifests dir: {}", e))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(manifest) = load_manifest(&path) {
            items.push(GameSnapshotSummary::from(&manifest));
        }
    }
    items.sort_by(|a, b| b.created_at.cmp(&a.created_at).then_with(|| b.id.cmp(&a.id)));
    Ok(items)
}

#[allow(dead_code)]
pub fn restore_snapshot_in_dir(
    game_dir: &Path,
    cas_dir: &Path,
    snapshot_id: &str,
) -> Result<SnapshotRestoreReport, String> {
    restore_snapshot_in_dir_filtered(game_dir, cas_dir, snapshot_id, None)
}

pub fn restore_snapshot_in_dir_filtered(
    game_dir: &Path,
    cas_dir: &Path,
    snapshot_id: &str,
    include_subdirs: Option<&[&str]>,
) -> Result<SnapshotRestoreReport, String> {
    let started = Instant::now();
    let manifest_path = cas_dir
        .join("manifests")
        .join(format!("{}.json", snapshot_id));
    if !manifest_path.is_file() {
        return Err(format!("Snapshot '{}' was not found.", snapshot_id));
    }
    let manifest = load_manifest(&manifest_path)?;

    let mut target_map: HashMap<String, &SnapshotFileRecord> =
        HashMap::with_capacity(manifest.files.len());
    for rec in &manifest.files {
        let blob_path = blob_path_for_hash(cas_dir, &rec.blake3);
        if !blob_path.is_file() {
            return Err(format!(
                "Snapshot blob {} missing for '{}'.",
                rec.blake3, rec.rel_path
            ));
        }
        target_map.insert(rec.rel_path.clone(), rec);
    }

    let mut removed_files = 0usize;
    let mut dirs_to_prune = Vec::new();
    for entry in WalkDir::new(game_dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !should_skip_walk_entry(e, game_dir))
    {
        let entry = match entry {
            Ok(v) => v,
            Err(_) => continue,
        };
        if entry.path() == game_dir {
            continue;
        }
        if entry.file_type().is_dir() {
            let rel_dir = normalize_rel_path(game_dir, entry.path()).unwrap_or_default();
            if rel_matches_subdirs(&rel_dir, include_subdirs) {
                dirs_to_prune.push(entry.path().to_path_buf());
            }
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let rel_path = normalize_rel_path(game_dir, entry.path())?;
        if rel_matches_subdirs(&rel_path, include_subdirs) && !target_map.contains_key(&rel_path) {
            clear_readonly_if_present(entry.path());
            if fs::remove_file(entry.path()).is_ok() {
                removed_files += 1;
            }
        }
    }

    let mut unchanged_files = 0usize;
    let mut restored_files = 0usize;
    let mut hardlinked_files = 0usize;
    let mut copied_files = 0usize;

    for rec in &manifest.files {
        let target_path = safe_join_rel(game_dir, &rec.rel_path)?;
        let blob_path = blob_path_for_hash(cas_dir, &rec.blake3);

        let already_matches = fs::metadata(&target_path)
            .ok()
            .filter(|m| m.is_file() && m.len() == rec.size_bytes)
            .and_then(|_| hash_file_blake3_64k(&target_path).ok())
            .map(|h| h == rec.blake3)
            .unwrap_or(false);

        if already_matches {
            unchanged_files += 1;
            continue;
        }

        let linked = link_or_copy_from_blob(&target_path, &blob_path, rec.size_bytes)?;
        restored_files += 1;
        if linked {
            hardlinked_files += 1;
        } else {
            copied_files += 1;
        }
    }

    dirs_to_prune.sort_by_key(|a| std::cmp::Reverse(a.components().count()));
    for dir in dirs_to_prune {
        if let Ok(mut rd) = fs::read_dir(&dir) {
            if rd.next().is_none() {
                let _ = fs::remove_dir(&dir);
            }
        }
    }

    save_stat_index(cas_dir, &manifest.files);

    Ok(SnapshotRestoreReport {
        snapshot_id: manifest.id,
        label: manifest.label,
        restored_files,
        unchanged_files,
        hardlinked_files,
        copied_files,
        removed_files,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

/// Removes unreferenced blobs inside `.duke-cas/blobs/` and returns `(blobs_removed, bytes_freed)`.
pub fn gc_cas_blobs(cas_dir: &Path) -> Result<(usize, u64), String> {
    let mut referenced = HashSet::new();
    let manifests_dir = cas_dir.join("manifests");
    if manifests_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&manifests_dir) {
            for entry in entries.flatten() {
                if let Ok(m) = load_manifest(&entry.path()) {
                    for file in m.files {
                        referenced.insert(file.blake3);
                    }
                }
            }
        }
    }

    let mut blobs_removed = 0usize;
    let mut bytes_freed = 0u64;
    let blobs_dir = cas_dir.join("blobs");
    if blobs_dir.is_dir() {
        for entry in WalkDir::new(&blobs_dir).into_iter().flatten() {
            if !entry.file_type().is_file() {
                continue;
            }
            let Some(stem) = entry.path().file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if !referenced.contains(stem) {
                let sz = entry.metadata().map(|m| m.len()).unwrap_or(0);
                if fs::remove_file(entry.path()).is_ok() {
                    blobs_removed += 1;
                    bytes_freed = bytes_freed.saturating_add(sz);
                }
            }
        }
    }

    Ok((blobs_removed, bytes_freed))
}

/// Keeps at most `max_count` newest snapshots inside `cas_dir/manifests/`, returning the count deleted.
pub fn prune_snapshots_to_limit(cas_dir: &Path, max_count: usize) -> Result<usize, String> {
    let keep = max_count.max(1);
    let summaries = list_snapshots_in_cas(cas_dir)?;
    let mut removed = 0usize;
    for old in summaries.into_iter().skip(keep) {
        let manifest_path = cas_dir.join("manifests").join(format!("{}.json", old.id));
        if manifest_path.is_file() && fs::remove_file(&manifest_path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

pub fn delete_snapshot_in_cas(cas_dir: &Path, snapshot_id: &str) -> Result<Vec<GameSnapshotSummary>, String> {
    let manifest_path = cas_dir
        .join("manifests")
        .join(format!("{}.json", snapshot_id));
    if manifest_path.is_file() {
        fs::remove_file(&manifest_path)
            .map_err(|e| format!("Failed to delete snapshot manifest: {}", e))?;
    }

    let _ = gc_cas_blobs(cas_dir)?;
    list_snapshots_in_cas(cas_dir)
}

#[tauri::command]
pub async fn create_game_snapshot(
    game_id: String,
    label: Option<String>,
) -> Result<GameSnapshotSummary, String> {
    let (game_dir, include_subdirs) = resolve_snapshot_scope_by_id(&game_id)?;
    let cas_dir = game_dir.join(CAS_DIR_NAME);
    let max_keep = crate::load_settings_standalone()
        .max_restore_points
        .clamp(1, 50) as usize;
    let label_owned = label;
    tokio::task::spawn_blocking(move || {
        let sub_slice: Option<&[&str]> = include_subdirs.as_deref();
        let manifest = create_snapshot_in_dir_filtered(
            &game_id,
            &game_dir,
            &cas_dir,
            label_owned.as_deref(),
            sub_slice,
        )?;
        if prune_snapshots_to_limit(&cas_dir, max_keep).unwrap_or(0) > 0 {
            let _ = gc_cas_blobs(&cas_dir);
        }
        Ok(GameSnapshotSummary::from(&manifest))
    })
    .await
    .map_err(|e| format!("Snapshot worker failed: {}", e))?
}

#[tauri::command]
pub async fn reclaim_snapshot_space() -> Result<SnapshotGcReport, String> {
    let library = crate::load_library_standalone()?;
    let max_keep = crate::load_settings_standalone()
        .max_restore_points
        .clamp(1, 50) as usize;
    tokio::task::spawn_blocking(move || {
        let mut seen_cas_dirs = HashSet::new();
        let mut games_scanned = 0usize;
        let mut snapshots_pruned = 0usize;
        let mut blobs_removed = 0usize;
        let mut bytes_freed = 0u64;

        for entry in &library {
            let game_dir = if !entry.install_dir.trim().is_empty()
                && Path::new(&entry.install_dir).is_dir()
            {
                PathBuf::from(&entry.install_dir)
            } else if !entry.exe_path.trim().is_empty() {
                crate::resolve_game_root_dir(Path::new(&entry.exe_path))
            } else {
                continue;
            };
            let cas_dir = game_dir.join(CAS_DIR_NAME);
            if !cas_dir.is_dir() || !seen_cas_dirs.insert(cas_dir.clone()) {
                continue;
            }
            games_scanned += 1;
            snapshots_pruned += prune_snapshots_to_limit(&cas_dir, max_keep).unwrap_or(0);
            if let Ok((removed, freed)) = gc_cas_blobs(&cas_dir) {
                blobs_removed += removed;
                bytes_freed = bytes_freed.saturating_add(freed);
            }
        }

        Ok(SnapshotGcReport {
            games_scanned,
            snapshots_pruned,
            blobs_removed,
            bytes_freed,
            freed_formatted: crate::format_bytes(bytes_freed),
        })
    })
    .await
    .map_err(|e| format!("Restore point cleanup worker failed: {}", e))?
}

#[tauri::command]
pub async fn list_game_snapshots(game_id: String) -> Result<Vec<GameSnapshotSummary>, String> {
    let game_dir = resolve_game_dir_by_id(&game_id)?;
    let cas_dir = game_dir.join(CAS_DIR_NAME);
    tokio::task::spawn_blocking(move || list_snapshots_in_cas(&cas_dir))
        .await
        .map_err(|e| format!("Snapshot list worker failed: {}", e))?
}

#[tauri::command]
pub async fn restore_game_snapshot(
    game_id: String,
    snapshot_id: String,
) -> Result<SnapshotRestoreReport, String> {
    let (game_dir, include_subdirs) = resolve_snapshot_scope_by_id(&game_id)?;
    let cas_dir = game_dir.join(CAS_DIR_NAME);
    tokio::task::spawn_blocking(move || {
        restore_snapshot_in_dir_filtered(
            &game_dir,
            &cas_dir,
            &snapshot_id,
            include_subdirs.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("Snapshot restore worker failed: {}", e))?
}

#[tauri::command]
pub async fn delete_game_snapshot(
    game_id: String,
    snapshot_id: String,
) -> Result<Vec<GameSnapshotSummary>, String> {
    let game_dir = resolve_game_dir_by_id(&game_id)?;
    let cas_dir = game_dir.join(CAS_DIR_NAME);
    tokio::task::spawn_blocking(move || delete_snapshot_in_cas(&cas_dir, &snapshot_id))
        .await
        .map_err(|e| format!("Snapshot delete worker failed: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("duke_snap_test_{}_{}", tag, now_unix_millis()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn cas_snapshot_dedup_and_restore_preserves_state() {
        let root = temp_dir("basic");
        let game_dir = root.join("Game");
        let cas_dir = game_dir.join(CAS_DIR_NAME);
        fs::create_dir_all(game_dir.join("data")).unwrap();

        fs::write(game_dir.join("config.ini"), b"quality=ultra\n").unwrap();
        let big_payload = vec![0x5Au8; 1_200_000];
        fs::write(game_dir.join("data/assets_a.pak"), &big_payload).unwrap();
        fs::write(game_dir.join("data/assets_b.pak"), &big_payload).unwrap();

        let snap1 =
            create_snapshot_in_dir("g1", &game_dir, &cas_dir, Some("Clean Install")).unwrap();
        assert_eq!(snap1.file_count, 3);
        assert_eq!(snap1.unique_blobs_added, 2);
        assert_eq!(snap1.deduplicated_files, 1);

        // Wait 1ms & mutate files, add mod file, and delete assets_b.pak.
        fs::write(game_dir.join("config.ini"), b"quality=modded\n").unwrap();
        fs::write(game_dir.join("mod.dll"), b"mod-bytes").unwrap();
        fs::remove_file(game_dir.join("data/assets_b.pak")).unwrap();

        let snap2 =
            create_snapshot_in_dir("g1", &game_dir, &cas_dir, Some("Modded")).unwrap();
        assert_eq!(snap2.file_count, 3);

        // Roll back to Clean Install.
        let report = restore_snapshot_in_dir(&game_dir, &cas_dir, &snap1.id).unwrap();
        assert_eq!(report.removed_files, 1);
        assert_eq!(report.hardlinked_files, 1);
        assert!(!game_dir.join("mod.dll").exists());
        assert_eq!(
            fs::read(game_dir.join("config.ini")).unwrap(),
            b"quality=ultra\n"
        );
        assert_eq!(
            fs::read(game_dir.join("data/assets_b.pak")).unwrap().len(),
            1_200_000
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_snapshot_garbage_collects_unreferenced_blobs() {
        let root = temp_dir("gc");
        let game_dir = root.join("Game");
        let cas_dir = game_dir.join(CAS_DIR_NAME);
        fs::create_dir_all(&game_dir).unwrap();

        fs::write(game_dir.join("save.dat"), b"state-v1").unwrap();
        let snap1 = create_snapshot_in_dir("g2", &game_dir, &cas_dir, Some("v1")).unwrap();
        let hash1 = snap1.files[0].blake3.clone();
        assert!(blob_path_for_hash(&cas_dir, &hash1).is_file());

        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(game_dir.join("save.dat"), b"state-v2-different").unwrap();
        let snap2 = create_snapshot_in_dir("g2", &game_dir, &cas_dir, Some("v2")).unwrap();
        let hash2 = snap2.files[0].blake3.clone();
        assert_ne!(hash1, hash2);

        let remaining = delete_snapshot_in_cas(&cas_dir, &snap1.id).unwrap();
        assert_eq!(remaining.len(), 1);
        assert!(!blob_path_for_hash(&cas_dir, &hash1).exists());
        assert!(blob_path_for_hash(&cas_dir, &hash2).is_file());

        let _ = fs::remove_dir_all(&root);
    }
}
