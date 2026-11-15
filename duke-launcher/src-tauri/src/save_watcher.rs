//! Real-time Win32 `ReadDirectoryChangesW` save-file watcher & deduplicated `.duke-cas/` auto-snapshotter.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tauri::Emitter;
use walkdir::WalkDir;

use crate::snapshots::{self, GameSnapshotManifest, CAS_DIR_NAME, PROFILE_DIR_NAME};

const DEFAULT_DEBOUNCE_MS: u64 = 2_000;
const HASH_CHUNK_BYTES: usize = 64 * 1024;
const WATCH_BUFFER_WORDS: usize = 4096; // 16 KiB, 4-byte aligned heap buffer
const AUTO_WATCHER_LABEL: &str = "auto-save-watcher";
const AUTO_WATCHER_STATE_FILE: &str = "auto_watcher_state.json";
const MAX_TRANSIENT_SNAPSHOT_RETRIES: u8 = 3;

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaveWatcherStatus {
    pub active_game_id: Option<String>,
    pub watched_dirs: Vec<String>,
    pub snapshots_taken: u64,
    pub dedup_skips: u64,
    pub last_snapshot_id: Option<String>,
    pub last_snapshot_ts: Option<u64>,
    pub last_tree_hash: Option<String>,
    pub debounce_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct AutoWatcherDiskState {
    game_id: String,
    #[serde(
        rename = "auto-save-watcher-last-hash",
        alias = "autoSaveWatcherLastHash",
        alias = "lastTreeHash"
    )]
    last_tree_hash: String,
    last_snapshot_id: Option<String>,
    last_snapshot_ts: Option<u64>,
    file_count: usize,
    total_bytes: u64,
    snapshots_taken: u64,
    dedup_skips: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveWatcherSnapshotEvent {
    game_id: String,
    snapshot_id: String,
    label: String,
    created_at: u64,
    file_count: usize,
    total_bytes: u64,
    unique_blobs_added: usize,
    deduplicated_files: usize,
    tree_hash: String,
    cas_dir: String,
}

struct WatcherEntry {
    watched_dirs: Vec<String>,
    debounce_ms: u64,
    stop_tx: mpsc::Sender<()>,
    #[cfg(target_os = "windows")]
    stop_event_raw: usize,
    join_handle: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct WatcherRuntimeState {
    status: SaveWatcherStatus,
    ram_hashes: HashMap<String, AutoWatcherDiskState>,
}

static WATCHERS_MAP: OnceLock<Mutex<HashMap<String, WatcherEntry>>> = OnceLock::new();
static RUNTIME_STATE: OnceLock<Mutex<WatcherRuntimeState>> = OnceLock::new();
static SNAPSHOT_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static STAGING_SEQ: AtomicU64 = AtomicU64::new(1);

fn watchers_map() -> &'static Mutex<HashMap<String, WatcherEntry>> {
    WATCHERS_MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

fn runtime_state() -> &'static Mutex<WatcherRuntimeState> {
    RUNTIME_STATE.get_or_init(|| {
        Mutex::new(WatcherRuntimeState {
            status: SaveWatcherStatus {
                debounce_ms: DEFAULT_DEBOUNCE_MS,
                ..Default::default()
            },
            ram_hashes: HashMap::new(),
        })
    })
}

fn snapshot_io_lock() -> &'static Mutex<()> {
    SNAPSHOT_IO_LOCK.get_or_init(|| Mutex::new(()))
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct StagingDirGuard(PathBuf);

impl Drop for StagingDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn canonical_clean_dir(path: &Path) -> Option<PathBuf> {
    if !path.is_dir() {
        return None;
    }
    let canon = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    #[cfg(target_os = "windows")]
    {
        let s = canon.to_string_lossy();
        if let Some(unc_rest) = s.strip_prefix(r"\\?\UNC\") {
            return Some(PathBuf::from(format!(r"\\{}", unc_rest)));
        }
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            return Some(PathBuf::from(rest));
        }
    }
    Some(canon)
}

fn canonical_path_key(path: &Path) -> String {
    let resolved = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let s = resolved.to_string_lossy();
    #[cfg(target_os = "windows")]
    {
        let trimmed = s.strip_prefix(r"\\?\").unwrap_or(&s);
        trimmed.trim_end_matches(['\\', '/']).to_ascii_lowercase()
    }
    #[cfg(not(target_os = "windows"))]
    {
        s.trim_end_matches('/').to_string()
    }
}

fn dedup_canonical_dirs(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for p in paths {
        if let Some(clean) = canonical_clean_dir(p) {
            let key = canonical_path_key(&clean);
            if !key.is_empty() && seen.insert(key) {
                out.push(clean);
            }
        }
    }
    out
}

fn dedup_canonical_roots_any(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for p in paths {
        if !p.exists() {
            continue;
        }
        let clean = if p.is_dir() {
            canonical_clean_dir(p).unwrap_or_else(|| p.clone())
        } else {
            let canon = fs::canonicalize(p).unwrap_or_else(|_| p.clone());
            #[cfg(target_os = "windows")]
            {
                let s = canon.to_string_lossy();
                if let Some(unc_rest) = s.strip_prefix(r"\\?\UNC\") {
                    PathBuf::from(format!(r"\\{}", unc_rest))
                } else if let Some(rest) = s.strip_prefix(r"\\?\") {
                    PathBuf::from(rest)
                } else {
                    canon
                }
            }
            #[cfg(not(target_os = "windows"))]
            canon
        };
        let key = canonical_path_key(&clean);
        if !key.is_empty() && seen.insert(key) {
            out.push(clean);
        }
    }
    out
}

fn resolve_game_root_for_entry(game: &crate::GameEntry) -> Option<PathBuf> {
    let install_trimmed = game.install_dir.trim();
    if !install_trimmed.is_empty() {
        let p = PathBuf::from(install_trimmed);
        if p.is_dir() {
            return Some(p);
        }
    }
    let exe_trimmed = game.exe_path.trim();
    if !exe_trimmed.is_empty() {
        let exe_path = Path::new(exe_trimmed);
        let candidate = if exe_path.is_file() || exe_path.is_dir() {
            crate::resolve_game_root_dir(exe_path)
        } else {
            let parent = exe_path.parent().filter(|p| p.is_dir())?;
            crate::resolve_game_root_dir(parent)
        };
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

fn resolve_save_roots_for_game(game: &crate::GameEntry) -> Vec<PathBuf> {
    let cache = crate::ludusavi::load_ludusavi_cache_from_disk();
    let report = crate::ludusavi::build_save_report_for_entry(game, &cache);

    let mut raw_dirs = Vec::new();
    for cand in &report.existing_paths {
        let trimmed = cand.path.trim();
        if trimmed.is_empty() {
            continue;
        }
        let p = PathBuf::from(trimmed);
        if p.is_dir() {
            raw_dirs.push(p);
        } else if p.is_file() {
            if let Some(parent) = p.parent() {
                if parent.is_dir() {
                    raw_dirs.push(parent.to_path_buf());
                }
            }
        }
    }

    if let Some(game_root) = resolve_game_root_for_entry(game) {
        let profile_dir = game_root.join(PROFILE_DIR_NAME);
        if profile_dir.is_dir() {
            raw_dirs.push(profile_dir);
        }
    }

    dedup_canonical_dirs(&raw_dirs)
}

fn is_dir_writable(dir: &Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    let seq = STAGING_SEQ.fetch_add(1, Ordering::Relaxed);
    let probe = dir.join(format!(".duke-write-probe-{}-{}", std::process::id(), seq));
    match OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(mut f) => {
            let ok = f.write_all(b"1").is_ok();
            drop(f);
            let _ = fs::remove_file(&probe);
            ok
        }
        Err(_) => false,
    }
}

fn safe_game_id_segment(game_id: &str) -> String {
    let trimmed = game_id.trim();
    let cleaned: String = trimmed
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches('_');
    if cleaned.is_empty() {
        blake3::hash(trimmed.as_bytes()).to_hex()[..16].to_string()
    } else {
        cleaned.chars().take(64).collect()
    }
}

fn resolve_cas_dir_for_game(game: &crate::GameEntry) -> Result<PathBuf, String> {
    if let Some(game_root) = resolve_game_root_for_entry(game) {
        let preferred_cas = game_root.join(CAS_DIR_NAME);
        let writable = if preferred_cas.exists() {
            is_dir_writable(&preferred_cas)
        } else {
            is_dir_writable(&game_root)
        };
        if writable {
            return Ok(preferred_cas);
        }
    }

    let data_root = crate::resolve_data_root_dir_standalone()?;
    Ok(data_root
        .join("save_cas")
        .join(safe_game_id_segment(&game.id))
        .join(CAS_DIR_NAME))
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
                    "Unsafe path component in save entry: {}",
                    rel.display()
                ));
            }
        }
    }
    Ok(parts.join("/"))
}

fn should_skip_save_walk(entry: &walkdir::DirEntry, root: &Path) -> bool {
    if entry.path() == root {
        return false;
    }
    let Some(name) = entry.file_name().to_str() else {
        return false;
    };
    name.eq_ignore_ascii_case(CAS_DIR_NAME) || name.eq_ignore_ascii_case(".duke-patch-tmp")
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn compute_save_tree_blake3(roots: &[PathBuf]) -> Result<(String, usize, u64), String> {
    let existing_roots = dedup_canonical_roots_any(roots);
    struct FileWorkItem {
        root_idx: usize,
        rel_path: String,
        full_path: PathBuf,
    }

    let mut items = Vec::new();
    for (root_idx, root) in existing_roots.iter().enumerate() {
        if root.is_file() {
            let name = root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "save.bin".to_string());
            items.push(FileWorkItem {
                root_idx,
                rel_path: name,
                full_path: root.clone(),
            });
            continue;
        }
        if !root.is_dir() {
            continue;
        }
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !should_skip_save_walk(e, root))
        {
            let entry = match entry {
                Ok(v) => v,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let rel_path = normalize_rel_path(root, entry.path())?;
            if rel_path.is_empty() {
                continue;
            }
            items.push(FileWorkItem {
                root_idx,
                rel_path,
                full_path: entry.path().to_path_buf(),
            });
        }
    }

    items.sort_by(|a, b| {
        a.root_idx
            .cmp(&b.root_idx)
            .then_with(|| a.rel_path.cmp(&b.rel_path))
    });

    let mut tree_hasher = blake3::Hasher::new();
    tree_hasher.update(b"DUKE-SAVE-TREE-V1\0");
    tree_hasher.update(&(items.len() as u64).to_le_bytes());

    let mut buf = vec![0u8; HASH_CHUNK_BYTES];
    let mut total_bytes: u64 = 0;

    for item in &items {
        let mut file = File::open(&item.full_path)
            .map_err(|e| format!("Failed to open {}: {}", item.full_path.display(), e))?;
        let mut file_hasher = blake3::Hasher::new();
        let mut file_bytes: u64 = 0;
        loop {
            let n = file
                .read(&mut buf)
                .map_err(|e| format!("Failed to read {}: {}", item.full_path.display(), e))?;
            if n == 0 {
                break;
            }
            file_bytes = file_bytes.saturating_add(n as u64);
            file_hasher.update(&buf[..n]);
        }
        let digest = file_hasher.finalize();
        total_bytes = total_bytes.saturating_add(file_bytes);

        tree_hasher.update(&(item.root_idx as u32).to_le_bytes());
        tree_hasher.update(&(item.rel_path.len() as u64).to_le_bytes());
        tree_hasher.update(item.rel_path.as_bytes());
        tree_hasher.update(&file_bytes.to_le_bytes());
        tree_hasher.update(digest.as_bytes());
    }

    Ok((
        tree_hasher.finalize().to_hex().to_string(),
        items.len(),
        total_bytes,
    ))
}

fn stream_copy_64k(src: &Path, dst: &Path) -> Result<(), String> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create staging dir {}: {}", parent.display(), e))?;
    }
    let mut reader =
        File::open(src).map_err(|e| format!("Failed to open {}: {}", src.display(), e))?;
    let mut writer =
        File::create(dst).map_err(|e| format!("Failed to create {}: {}", dst.display(), e))?;
    let mut buf = vec![0u8; HASH_CHUNK_BYTES];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("Failed to read {}: {}", src.display(), e))?;
        if n == 0 {
            break;
        }
        writer
            .write_all(&buf[..n])
            .map_err(|e| format!("Failed to write {}: {}", dst.display(), e))?;
    }
    Ok(())
}

fn link_or_copy_to_staging(src: &Path, dst: &Path) -> Result<(), String> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create staging dir {}: {}", parent.display(), e))?;
    }
    if duke_core::win32_ffi::create_hard_link(dst, src).is_ok() {
        return Ok(());
    }
    stream_copy_64k(src, dst)
}

fn create_multi_root_snapshot(
    game_id: &str,
    roots: &[PathBuf],
    cas_dir: &Path,
    label: &str,
) -> Result<GameSnapshotManifest, String> {
    if roots.len() == 1 && roots[0].is_dir() {
        return snapshots::create_snapshot_in_dir(game_id, &roots[0], cas_dir, Some(label));
    }

    fs::create_dir_all(cas_dir)
        .map_err(|e| format!("Failed to create CAS root {}: {}", cas_dir.display(), e))?;
    let seq = STAGING_SEQ.fetch_add(1, Ordering::Relaxed);
    let staging_dir = cas_dir.join(format!(".staging-{}-{}", std::process::id(), seq));
    fs::create_dir_all(&staging_dir)
        .map_err(|e| format!("Failed to create snapshot staging dir: {}", e))?;
    let _guard = StagingDirGuard(staging_dir.clone());

    for (idx, root) in roots.iter().enumerate() {
        let sub_root = staging_dir.join(format!("root_{}", idx));
        fs::create_dir_all(&sub_root)
            .map_err(|e| format!("Failed to create staging subdir: {}", e))?;
        if root.is_file() {
            let fname = root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "save.bin".to_string());
            link_or_copy_to_staging(root, &sub_root.join(fname))?;
            continue;
        }
        if !root.is_dir() {
            continue;
        }
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !should_skip_save_walk(e, root))
        {
            let entry = match entry {
                Ok(v) => v,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = normalize_rel_path(root, entry.path())?;
            if rel.is_empty() {
                continue;
            }
            let mut dst = sub_root.clone();
            for part in rel.split('/') {
                if !part.is_empty() {
                    dst.push(part);
                }
            }
            link_or_copy_to_staging(entry.path(), &dst)?;
        }
    }

    snapshots::create_snapshot_in_dir(game_id, &staging_dir, cas_dir, Some(label))
}

fn load_disk_watcher_state(cas_dir: &Path) -> Option<AutoWatcherDiskState> {
    let state_path = cas_dir.join(AUTO_WATCHER_STATE_FILE);
    let raw = fs::read_to_string(&state_path).ok()?;
    serde_json::from_str::<AutoWatcherDiskState>(&raw).ok()
}

fn save_disk_watcher_state(cas_dir: &Path, state: &AutoWatcherDiskState) -> Result<(), String> {
    fs::create_dir_all(cas_dir)
        .map_err(|e| format!("Failed to create CAS directory {}: {}", cas_dir.display(), e))?;
    let serialized = serde_json::to_string_pretty(state)
        .map_err(|e| format!("Failed to serialize auto watcher state: {}", e))?;
    crate::atomic_write_string(&cas_dir.join(AUTO_WATCHER_STATE_FILE), &serialized)
}

fn cas_manifest_exists(cas_dir: &Path, snap_id: Option<&str>) -> bool {
    let Some(id) = snap_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    cas_dir
        .join("manifests")
        .join(format!("{}.json", id))
        .is_file()
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn capture_dedup_save_snapshot(
    game_id: &str,
    save_roots: &[PathBuf],
    cas_dir: &Path,
    label: Option<&str>,
) -> Result<Option<GameSnapshotManifest>, String> {
    let _io_guard = lock_unpoisoned(snapshot_io_lock());

    let existing_roots = dedup_canonical_roots_any(save_roots);
    if existing_roots.is_empty() {
        return Err("No existing save directories to snapshot.".to_string());
    }

    let (tree_hash, file_count, total_bytes) = compute_save_tree_blake3(&existing_roots)?;
    let cas_key = canonical_path_key(cas_dir);
    let state_path = cas_dir.join(AUTO_WATCHER_STATE_FILE);
    let disk_state = load_disk_watcher_state(cas_dir);

    let prev_state = {
        let rt = lock_unpoisoned(runtime_state());
        if state_path.is_file() {
            rt.ram_hashes.get(&cas_key).cloned().or(disk_state)
        } else {
            disk_state
        }
    };

    if file_count == 0 && prev_state.is_none() {
        return Ok(None);
    }

    if let Some(mut prev) = prev_state.clone() {
        let manifest_present = cas_manifest_exists(cas_dir, prev.last_snapshot_id.as_deref());
        if !prev.last_tree_hash.is_empty() && prev.last_tree_hash == tree_hash && manifest_present
        {
            prev.dedup_skips = prev.dedup_skips.saturating_add(1);
            let _ = save_disk_watcher_state(cas_dir, &prev);

            let mut rt = lock_unpoisoned(runtime_state());
            rt.ram_hashes.insert(cas_key, prev);
            if rt.status.active_game_id.as_deref() == Some(game_id) {
                rt.status.dedup_skips = rt.status.dedup_skips.saturating_add(1);
                rt.status.last_tree_hash = Some(tree_hash);
            }
            return Ok(None);
        }
    }

    let eff_label = label
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(AUTO_WATCHER_LABEL);

    let manifest = create_multi_root_snapshot(game_id, &existing_roots, cas_dir, eff_label)?;

    let next_state = AutoWatcherDiskState {
        game_id: game_id.to_string(),
        last_tree_hash: tree_hash.clone(),
        last_snapshot_id: Some(manifest.id.clone()),
        last_snapshot_ts: Some(manifest.created_at),
        file_count,
        total_bytes,
        snapshots_taken: prev_state
            .as_ref()
            .map(|s| s.snapshots_taken.saturating_add(1))
            .unwrap_or(1),
        dedup_skips: prev_state.as_ref().map(|s| s.dedup_skips).unwrap_or(0),
    };
    save_disk_watcher_state(cas_dir, &next_state)?;

    let mut rt = lock_unpoisoned(runtime_state());
    rt.ram_hashes.insert(cas_key, next_state);
    if rt.status.active_game_id.as_deref() == Some(game_id) {
        rt.status.snapshots_taken = rt.status.snapshots_taken.saturating_add(1);
        rt.status.last_snapshot_id = Some(manifest.id.clone());
        rt.status.last_snapshot_ts = Some(manifest.created_at);
        rt.status.last_tree_hash = Some(tree_hash);
    }

    Ok(Some(manifest))
}

#[cfg(target_os = "windows")]
#[inline]
fn read_u32_le(words: &[u32], byte_offset: usize) -> u32 {
    let mut b = [0u8; 4];
    for (i, slot) in b.iter_mut().enumerate() {
        let idx = byte_offset + i;
        let word = words[idx / 4];
        *slot = word.to_le_bytes()[idx % 4];
    }
    u32::from_le_bytes(b)
}

#[cfg(target_os = "windows")]
#[inline]
fn read_u16_le(words: &[u32], byte_offset: usize) -> u16 {
    let lo = words[byte_offset / 4].to_le_bytes()[byte_offset % 4];
    let hi = words[(byte_offset + 1) / 4].to_le_bytes()[(byte_offset + 1) % 4];
    u16::from_le_bytes([lo, hi])
}

#[cfg(target_os = "windows")]
fn parse_notify_filenames(words: &[u32], bytes_transferred: usize) -> Vec<String> {
    let limit = bytes_transferred.min(std::mem::size_of_val(words));
    let mut names = Vec::new();
    let mut offset = 0usize;
    while offset.saturating_add(12) <= limit {
        let next_off = read_u32_le(words, offset) as usize;
        let name_len_bytes = read_u32_le(words, offset + 8) as usize;
        let name_start = offset + 12;
        let name_end = name_start.saturating_add(name_len_bytes);
        if name_end <= limit && name_len_bytes >= 2 {
            let mut wide = Vec::with_capacity(name_len_bytes / 2);
            let mut pos = name_start;
            while pos + 1 < name_end {
                wide.push(read_u16_le(words, pos));
                pos += 2;
            }
            names.push(String::from_utf16_lossy(&wide));
        }
        if next_off == 0 {
            break;
        }
        let next = offset.saturating_add(next_off);
        if next <= offset || next >= limit {
            break;
        }
        offset = next;
    }
    names
}

#[cfg(target_os = "windows")]
fn is_ignored_watch_rel_path(rel_name: &str) -> bool {
    rel_name.split(['\\', '/']).any(|seg| {
        seg.eq_ignore_ascii_case(CAS_DIR_NAME) || seg.eq_ignore_ascii_case(".duke-patch-tmp")
    })
}

#[cfg(target_os = "windows")]
fn has_relevant_save_change(words: &[u32], bytes_transferred: usize) -> bool {
    if bytes_transferred == 0 {
        return true;
    }
    let names = parse_notify_filenames(words, bytes_transferred);
    if names.is_empty() {
        return true;
    }
    names.iter().any(|name| !is_ignored_watch_rel_path(name))
}

struct WatchThreadCtx {
    app: Option<tauri::AppHandle>,
    game_id: String,
    save_roots: Vec<PathBuf>,
    cas_dir: PathBuf,
    debounce: Duration,
    stop_rx: mpsc::Receiver<()>,
    #[cfg(target_os = "windows")]
    stop_event_raw: usize,
}

#[cfg(target_os = "windows")]
fn run_win32_watch_loop(
    ctx: WatchThreadCtx,
    ready_tx: mpsc::SyncSender<Result<Vec<String>, String>>,
) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, ReadDirectoryChangesW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED,
        FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE,
        FILE_NOTIFY_CHANGE_SIZE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING,
    };
    use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
    use windows::Win32::System::Threading::{CreateEventW, ResetEvent, WaitForMultipleObjects};

    if ctx.stop_event_raw == 0 {
        let _ = ready_tx.send(Err("Failed to create Win32 manual-reset stop event.".to_string()));
        return;
    }

    struct DirWatchHandle {
        dir_label: String,
        dir_handle: HANDLE,
        io_event: HANDLE,
        overlapped: Box<OVERLAPPED>,
        buffer: Vec<u32>,
        active: bool,
    }

    let filter =
        FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_SIZE;

    let mut watches: Vec<DirWatchHandle> = Vec::new();
    for dir in ctx.save_roots.iter().take(63) {
        if !dir.is_dir() {
            continue;
        }
        let mut wide: Vec<u16> = OsStr::new(dir).encode_wide().collect();
        wide.push(0);
        let Ok(dh) = (unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                FILE_LIST_DIRECTORY.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                None,
            )
        }) else {
            continue;
        };
        let Ok(ev) = (unsafe { CreateEventW(None, true, false, None) }) else {
            unsafe {
                let _ = CloseHandle(dh);
            }
            continue;
        };
        let mut overlapped = Box::new(OVERLAPPED::default());
        overlapped.hEvent = ev;
        watches.push(DirWatchHandle {
            dir_label: dir.to_string_lossy().into_owned(),
            dir_handle: dh,
            io_event: ev,
            overlapped,
            buffer: vec![0u32; WATCH_BUFFER_WORDS],
            active: false,
        });
    }

    for item in &mut watches {
        unsafe {
            let _ = ResetEvent(item.io_event);
            *item.overlapped = OVERLAPPED::default();
            item.overlapped.hEvent = item.io_event;
            let buf_bytes = (item.buffer.len() * std::mem::size_of::<u32>()) as u32;
            let ok = ReadDirectoryChangesW(
                item.dir_handle,
                item.buffer.as_mut_ptr() as *mut std::ffi::c_void,
                buf_bytes,
                true,
                filter,
                None,
                Some(&mut *item.overlapped),
                None,
            )
            .is_ok();
            item.active = ok;
        }
    }

    // Close any watches whose initial ReadDirectoryChangesW failed before entering loop.
    let mut armed_watches = Vec::with_capacity(watches.len());
    for w in watches {
        if w.active {
            armed_watches.push(w);
        } else {
            unsafe {
                let _ = CloseHandle(w.io_event);
                let _ = CloseHandle(w.dir_handle);
            }
        }
    }

    if armed_watches.is_empty() {
        let _ = ready_tx.send(Err(
            "Failed to open ReadDirectoryChangesW handles on any save directory.".to_string(),
        ));
        return;
    }

    let watched_labels: Vec<String> = armed_watches.iter().map(|w| w.dir_label.clone()).collect();
    let _ = ready_tx.send(Ok(watched_labels));

    let stop_handle = HANDLE(ctx.stop_event_raw as *mut std::ffi::c_void);
    let mut last_event_at: Option<Instant> = None;
    let mut retry_count: u8 = 0;

    loop {
        if ctx.stop_rx.try_recv().is_ok() {
            break;
        }
        if armed_watches.iter().all(|w| !w.active) {
            break;
        }

        let wait_ms: u32 = match last_event_at {
            Some(ts) => {
                let elapsed = ts.elapsed();
                if elapsed >= ctx.debounce {
                    0
                } else {
                    ctx.debounce
                        .saturating_sub(elapsed)
                        .as_millis()
                        .clamp(1, u32::MAX as u128) as u32
                }
            }
            None => 5_000,
        };

        let trigger_snapshot = if wait_ms == 0 {
            true
        } else {
            let mut wait_handles = Vec::with_capacity(armed_watches.len() + 1);
            let mut watch_indices = Vec::with_capacity(armed_watches.len());
            wait_handles.push(stop_handle);
            for (idx, w) in armed_watches.iter().enumerate() {
                if w.active {
                    wait_handles.push(w.io_event);
                    watch_indices.push(idx);
                }
            }

            let res = unsafe { WaitForMultipleObjects(&wait_handles, false, wait_ms) };
            if res == WAIT_OBJECT_0 {
                break;
            }
            if res == WAIT_TIMEOUT {
                last_event_at.is_some_and(|ts| ts.elapsed() >= ctx.debounce)
            } else if res.0 > WAIT_OBJECT_0.0 {
                let slot = (res.0 - WAIT_OBJECT_0.0 - 1) as usize;
                if let Some(&watch_idx) = watch_indices.get(slot) {
                    let item = &mut armed_watches[watch_idx];
                    let mut bytes_transferred: u32 = 0;
                    let ov_ok = unsafe {
                        GetOverlappedResult(
                            item.dir_handle,
                            &*item.overlapped,
                            &mut bytes_transferred,
                            false,
                        )
                        .is_ok()
                    };
                    if ov_ok && has_relevant_save_change(&item.buffer, bytes_transferred as usize) {
                        last_event_at = Some(Instant::now());
                        retry_count = 0;
                    }
                    unsafe {
                        let _ = ResetEvent(item.io_event);
                        *item.overlapped = OVERLAPPED::default();
                        item.overlapped.hEvent = item.io_event;
                        let buf_bytes = (item.buffer.len() * std::mem::size_of::<u32>()) as u32;
                        let rearm_ok = ReadDirectoryChangesW(
                            item.dir_handle,
                            item.buffer.as_mut_ptr() as *mut std::ffi::c_void,
                            buf_bytes,
                            true,
                            filter,
                            None,
                            Some(&mut *item.overlapped),
                            None,
                        )
                        .is_ok();
                        if !rearm_ok {
                            item.active = false;
                        }
                    }
                }
                false
            } else {
                break;
            }
        };

        if trigger_snapshot {
            last_event_at = None;
            match capture_dedup_save_snapshot(
                &ctx.game_id,
                &ctx.save_roots,
                &ctx.cas_dir,
                Some(AUTO_WATCHER_LABEL),
            ) {
                Ok(Some(manifest)) => {
                    retry_count = 0;
                    let tree_hash = lock_unpoisoned(runtime_state())
                        .status
                        .last_tree_hash
                        .clone()
                        .unwrap_or_default();
                    if let Some(ref handle) = ctx.app {
                        let payload = SaveWatcherSnapshotEvent {
                            game_id: ctx.game_id.clone(),
                            snapshot_id: manifest.id.clone(),
                            label: manifest.label.clone(),
                            created_at: manifest.created_at,
                            file_count: manifest.file_count,
                            total_bytes: manifest.total_bytes,
                            unique_blobs_added: manifest.unique_blobs_added,
                            deduplicated_files: manifest.deduplicated_files,
                            tree_hash,
                            cas_dir: ctx.cas_dir.to_string_lossy().into_owned(),
                        };
                        let _ = handle.emit("save-watcher-snapshot", &payload);
                    }
                }
                Ok(None) => {
                    retry_count = 0;
                }
                Err(_) => {
                    if retry_count < MAX_TRANSIENT_SNAPSHOT_RETRIES {
                        retry_count += 1;
                        last_event_at = Some(Instant::now());
                    } else {
                        retry_count = 0;
                    }
                }
            }
        }
    }

    for item in &mut armed_watches {
        unsafe {
            let _ = CancelIoEx(item.dir_handle, Some(&*item.overlapped));
            let mut drained: u32 = 0;
            let _ = GetOverlappedResult(item.dir_handle, &*item.overlapped, &mut drained, true);
            let _ = CloseHandle(item.io_event);
            let _ = CloseHandle(item.dir_handle);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn run_win32_watch_loop(
    ctx: WatchThreadCtx,
    ready_tx: mpsc::SyncSender<Result<Vec<String>, String>>,
) {
    let labels: Vec<String> = ctx
        .save_roots
        .iter()
        .filter(|p| p.is_dir())
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    if labels.is_empty() {
        let _ = ready_tx.send(Err("No existing save directories to watch.".to_string()));
        return;
    }
    let _ = ready_tx.send(Ok(labels));
    while ctx.stop_rx.recv_timeout(Duration::from_secs(5)).is_err() {}
}

fn shutdown_watcher_entry(mut entry: WatcherEntry) {
    let _ = entry.stop_tx.send(());
    #[cfg(target_os = "windows")]
    if entry.stop_event_raw != 0 {
        unsafe {
            use windows::Win32::Foundation::HANDLE;
            use windows::Win32::System::Threading::SetEvent;
            let _ = SetEvent(HANDLE(entry.stop_event_raw as *mut std::ffi::c_void));
        }
    }
    if let Some(handle) = entry.join_handle.take() {
        let _ = handle.join();
    }
    #[cfg(target_os = "windows")]
    if entry.stop_event_raw != 0 {
        unsafe {
            use windows::Win32::Foundation::{CloseHandle, HANDLE};
            let _ = CloseHandle(HANDLE(entry.stop_event_raw as *mut std::ffi::c_void));
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn start_dir_watch_loop_with_debounce(
    app: Option<tauri::AppHandle>,
    game_id: &str,
    save_roots: Vec<PathBuf>,
    cas_dir: PathBuf,
    debounce_ms: u64,
) -> Result<SaveWatcherStatus, String> {
    let gid = game_id.trim().to_string();
    if gid.is_empty() {
        return Err("Game ID cannot be empty.".to_string());
    }

    let watch_dirs = dedup_canonical_dirs(&save_roots);
    if watch_dirs.is_empty() {
        return Err(format!(
            "No existing save directories found to watch for '{}'.",
            gid
        ));
    }

    stop_save_watcher_for_game(&gid);

    let effective_debounce_ms = debounce_ms.max(1);
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    #[cfg(target_os = "windows")]
    let stop_event_raw: usize = unsafe {
        use windows::Win32::System::Threading::CreateEventW;
        CreateEventW(None, true, false, None)
            .map(|h| h.0 as usize)
            .unwrap_or(0)
    };

    let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<Vec<String>, String>>(1);

    let thread_gid = gid.clone();
    let thread_cas = cas_dir.clone();
    let thread_roots = watch_dirs.clone();
    let thread_name = format!("duke-save-watch-{}", safe_game_id_segment(&gid));

    let spawn_res = std::thread::Builder::new().name(thread_name).spawn(move || {
        run_win32_watch_loop(
            WatchThreadCtx {
                app,
                game_id: thread_gid,
                save_roots: thread_roots,
                cas_dir: thread_cas,
                debounce: Duration::from_millis(effective_debounce_ms),
                stop_rx,
                #[cfg(target_os = "windows")]
                stop_event_raw,
            },
            ready_tx,
        );
    });

    let join_handle = match spawn_res {
        Ok(h) => h,
        Err(e) => {
            #[cfg(target_os = "windows")]
            if stop_event_raw != 0 {
                unsafe {
                    use windows::Win32::Foundation::{CloseHandle, HANDLE};
                    let _ = CloseHandle(HANDLE(stop_event_raw as *mut std::ffi::c_void));
                }
            }
            return Err(format!("Failed spawning save watcher thread: {}", e));
        }
    };

    let watched_labels = match ready_rx.recv() {
        Ok(Ok(labels)) => labels,
        Ok(Err(err)) => {
            let _ = join_handle.join();
            #[cfg(target_os = "windows")]
            if stop_event_raw != 0 {
                unsafe {
                    use windows::Win32::Foundation::{CloseHandle, HANDLE};
                    let _ = CloseHandle(HANDLE(stop_event_raw as *mut std::ffi::c_void));
                }
            }
            return Err(err);
        }
        Err(e) => {
            let _ = join_handle.join();
            #[cfg(target_os = "windows")]
            if stop_event_raw != 0 {
                unsafe {
                    use windows::Win32::Foundation::{CloseHandle, HANDLE};
                    let _ = CloseHandle(HANDLE(stop_event_raw as *mut std::ffi::c_void));
                }
            }
            return Err(format!("Save watcher thread aborted during startup: {}", e));
        }
    };

    let existing_disk_state = load_disk_watcher_state(&cas_dir);
    let cas_key = canonical_path_key(&cas_dir);

    {
        let mut map = lock_unpoisoned(watchers_map());
        map.insert(
            gid.clone(),
            WatcherEntry {
                watched_dirs: watched_labels.clone(),
                debounce_ms: effective_debounce_ms,
                stop_tx,
                #[cfg(target_os = "windows")]
                stop_event_raw,
                join_handle: Some(join_handle),
            },
        );
    }

    let mut rt = lock_unpoisoned(runtime_state());
    if let Some(ref disk) = existing_disk_state {
        rt.ram_hashes.insert(cas_key, disk.clone());
    }
    rt.status = SaveWatcherStatus {
        active_game_id: Some(gid),
        watched_dirs: watched_labels,
        snapshots_taken: 0,
        dedup_skips: 0,
        last_snapshot_id: existing_disk_state
            .as_ref()
            .and_then(|d| d.last_snapshot_id.clone()),
        last_snapshot_ts: existing_disk_state
            .as_ref()
            .and_then(|d| d.last_snapshot_ts),
        last_tree_hash: existing_disk_state
            .as_ref()
            .map(|d| d.last_tree_hash.clone())
            .filter(|s| !s.is_empty()),
        debounce_ms: effective_debounce_ms,
    };
    Ok(rt.status.clone())
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn start_save_watcher_for_game(
    app: Option<tauri::AppHandle>,
    game: &crate::GameEntry,
) -> Result<SaveWatcherStatus, String> {
    let save_roots = resolve_save_roots_for_game(game);
    if save_roots.is_empty() {
        return Err(format!(
            "No existing save directories found to watch for '{}'.",
            game.title
        ));
    }
    let cas_dir = resolve_cas_dir_for_game(game)?;
    start_dir_watch_loop_with_debounce(app, &game.id, save_roots, cas_dir, DEFAULT_DEBOUNCE_MS)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn stop_save_watcher_for_game(game_id: &str) {
    let removed = {
        let mut map = lock_unpoisoned(watchers_map());
        map.remove(game_id)
    };
    if let Some(entry) = removed {
        shutdown_watcher_entry(entry);
    }

    let fallback = {
        let map = lock_unpoisoned(watchers_map());
        map.iter()
            .next()
            .map(|(k, v)| (k.clone(), v.watched_dirs.clone(), v.debounce_ms))
    };

    let mut rt = lock_unpoisoned(runtime_state());
    if rt.status.active_game_id.as_deref() == Some(game_id) {
        if let Some((next_gid, next_dirs, next_debounce)) = fallback {
            rt.status.active_game_id = Some(next_gid);
            rt.status.watched_dirs = next_dirs;
            rt.status.debounce_ms = next_debounce;
        } else {
            rt.status.active_game_id = None;
            rt.status.watched_dirs.clear();
            rt.status.debounce_ms = DEFAULT_DEBOUNCE_MS;
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn stop_all_save_watchers() {
    let entries: Vec<WatcherEntry> = {
        let mut map = lock_unpoisoned(watchers_map());
        map.drain().map(|(_, v)| v).collect()
    };

    // Signal all manual-reset stop events concurrently before joining threads.
    for entry in &entries {
        let _ = entry.stop_tx.send(());
        #[cfg(target_os = "windows")]
        if entry.stop_event_raw != 0 {
            unsafe {
                use windows::Win32::Foundation::HANDLE;
                use windows::Win32::System::Threading::SetEvent;
                let _ = SetEvent(HANDLE(entry.stop_event_raw as *mut std::ffi::c_void));
            }
        }
    }

    for mut entry in entries {
        if let Some(handle) = entry.join_handle.take() {
            let _ = handle.join();
        }
        #[cfg(target_os = "windows")]
        if entry.stop_event_raw != 0 {
            unsafe {
                use windows::Win32::Foundation::{CloseHandle, HANDLE};
                let _ = CloseHandle(HANDLE(entry.stop_event_raw as *mut std::ffi::c_void));
            }
        }
    }

    let mut rt = lock_unpoisoned(runtime_state());
    rt.status.active_game_id = None;
    rt.status.watched_dirs.clear();
    rt.status.debounce_ms = DEFAULT_DEBOUNCE_MS;
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn get_save_watcher_status() -> SaveWatcherStatus {
    let rt = lock_unpoisoned(runtime_state());
    let mut status = rt.status.clone();
    if status.debounce_ms == 0 {
        status.debounce_ms = DEFAULT_DEBOUNCE_MS;
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_DAEMON_LOCK: Mutex<()> = Mutex::new(());

    fn make_temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = STAGING_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "duke_save_watch_test_{}_{}_{}_{}",
            tag,
            std::process::id(),
            nanos,
            seq
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn dedup_save_snapshot_skips_unchanged_and_reuses_cas_blobs() {
        let root = make_temp_dir("dedup");
        let save_dir = root.join("saves");
        let cas_dir = root.join(CAS_DIR_NAME);
        fs::create_dir_all(&save_dir).unwrap();

        let shared_payload = vec![0x5Au8; 96 * 1024];
        fs::write(save_dir.join("slot1.sav"), b"HP=100;LVL=04").unwrap();
        fs::write(save_dir.join("world_state.bin"), &shared_payload).unwrap();

        let first = capture_dedup_save_snapshot(
            "g_dedup",
            std::slice::from_ref(&save_dir),
            &cas_dir,
            Some(AUTO_WATCHER_LABEL),
        )
        .unwrap();
        let m1 = first.expect("first snapshot should be recorded");
        assert_eq!(m1.label, AUTO_WATCHER_LABEL);
        assert_eq!(m1.file_count, 2);
        assert_eq!(m1.unique_blobs_added, 2);

        let unchanged_blob_hash = m1
            .files
            .iter()
            .find(|f| f.rel_path == "world_state.bin")
            .unwrap()
            .blake3
            .clone();
        let unchanged_blob_path = cas_dir
            .join("blobs")
            .join(&unchanged_blob_hash[..2])
            .join(format!("{}.blob", unchanged_blob_hash));
        assert!(unchanged_blob_path.is_file());

        // Second call without any file modification must skip creating a redundant manifest.
        let second = capture_dedup_save_snapshot(
            "g_dedup",
            std::slice::from_ref(&save_dir),
            &cas_dir,
            Some(AUTO_WATCHER_LABEL),
        )
        .unwrap();
        assert!(second.is_none());
        assert_eq!(snapshots::list_snapshots_in_cas(&cas_dir).unwrap().len(), 1);

        // Simulate fresh process by clearing RAM digest entry; disk auto_watcher_state.json still dedups.
        {
            let mut rt = lock_unpoisoned(runtime_state());
            rt.ram_hashes.remove(&canonical_path_key(&cas_dir));
        }
        let third_disk_dedup = capture_dedup_save_snapshot(
            "g_dedup",
            std::slice::from_ref(&save_dir),
            &cas_dir,
            Some(AUTO_WATCHER_LABEL),
        )
        .unwrap();
        assert!(third_disk_dedup.is_none());
        assert_eq!(snapshots::list_snapshots_in_cas(&cas_dir).unwrap().len(), 1);

        // Modify a single byte in slot1.sav while leaving world_state.bin untouched.
        fs::write(save_dir.join("slot1.sav"), b"HP=100;LVL=05").unwrap();
        let fourth = capture_dedup_save_snapshot(
            "g_dedup",
            std::slice::from_ref(&save_dir),
            &cas_dir,
            Some(AUTO_WATCHER_LABEL),
        )
        .unwrap();
        let m2 = fourth.expect("modified save must produce new snapshot");
        assert_ne!(m2.id, m1.id);
        assert_eq!(m2.file_count, 2);
        assert_eq!(m2.unique_blobs_added, 1);
        assert_eq!(m2.deduplicated_files, 1);
        assert!(unchanged_blob_path.is_file());
        assert_eq!(snapshots::list_snapshots_in_cas(&cas_dir).unwrap().len(), 2);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn live_win32_watcher_debounces_write_and_stops_cleanly() {
        let _daemon_guard = TEST_DAEMON_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        stop_all_save_watchers();

        let root = make_temp_dir("live_win32");
        let save_dir = root.join("profile_saves");
        let cas_dir = root.join(CAS_DIR_NAME);
        fs::create_dir_all(&save_dir).unwrap();

        let start_status = start_dir_watch_loop_with_debounce(
            None,
            "g_live_watch",
            vec![save_dir.clone()],
            cas_dir.clone(),
            180,
        )
        .unwrap();
        assert_eq!(start_status.active_game_id.as_deref(), Some("g_live_watch"));
        assert_eq!(start_status.debounce_ms, 180);
        assert_eq!(start_status.watched_dirs.len(), 1);

        // Burst two writes inside the 180ms quiet debounce window.
        fs::write(save_dir.join("autosave.sav"), b"STATE_STEP_1").unwrap();
        std::thread::sleep(Duration::from_millis(25));
        fs::write(save_dir.join("autosave.sav"), b"STATE_STEP_2_FINAL").unwrap();

        let deadline = Instant::now() + Duration::from_millis(2_000);
        let mut snaps = Vec::new();
        let mut running_status = get_save_watcher_status();
        while Instant::now() < deadline {
            snaps = snapshots::list_snapshots_in_cas(&cas_dir).unwrap_or_default();
            running_status = get_save_watcher_status();
            if !snaps.is_empty() && running_status.snapshots_taken >= 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }

        assert!(
            !snaps.is_empty(),
            "expected live ReadDirectoryChangesW watcher to create a .duke-cas snapshot"
        );
        assert_eq!(snaps[0].label, AUTO_WATCHER_LABEL);
        assert_eq!(
            running_status.active_game_id.as_deref(),
            Some("g_live_watch")
        );
        assert!(running_status.snapshots_taken >= 1);
        assert!(running_status.last_tree_hash.is_some());

        // Rewriting identical save bytes must trigger dedup skip with zero extra snapshots.
        let count_before_identical = snaps.len();
        let taken_before_identical = running_status.snapshots_taken;
        fs::write(save_dir.join("autosave.sav"), b"STATE_STEP_2_FINAL").unwrap();
        let dedup_deadline = Instant::now() + Duration::from_millis(1_500);
        while Instant::now() < dedup_deadline {
            running_status = get_save_watcher_status();
            if running_status.dedup_skips >= 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(
            running_status.dedup_skips >= 1,
            "identical rewrite should increment dedup_skips"
        );
        assert_eq!(running_status.snapshots_taken, taken_before_identical);
        assert_eq!(
            snapshots::list_snapshots_in_cas(&cas_dir).unwrap().len(),
            count_before_identical
        );

        let stop_started = Instant::now();
        stop_save_watcher_for_game("g_live_watch");
        assert!(
            stop_started.elapsed() < Duration::from_millis(500),
            "stop_save_watcher_for_game should stop immediately without hanging"
        );

        let stopped_status = get_save_watcher_status();
        assert!(stopped_status.active_game_id.is_none());
        assert!(stopped_status.watched_dirs.is_empty());

        // Proves all Win32 directory handles were closed cleanly before return.
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn start_save_watcher_resolves_profile_and_multi_root_dedup() {
        let _daemon_guard = TEST_DAEMON_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        stop_all_save_watchers();

        let root = make_temp_dir("entry_resolve");
        let game_dir = root.join("MyGame");
        let profile_dir = game_dir.join(PROFILE_DIR_NAME);
        let local_saves = game_dir.join("saves");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::create_dir_all(&local_saves).unwrap();
        fs::write(profile_dir.join("profile.sav"), b"PROFILE_DATA").unwrap();
        fs::write(local_saves.join("slot0.sav"), b"SLOT0_DATA").unwrap();

        let entry = crate::GameEntry {
            id: "g_resolve_1".to_string(),
            title: "Save Watcher Unit Game".to_string(),
            install_dir: game_dir.to_string_lossy().into_owned(),
            exe_path: game_dir.join("game.exe").to_string_lossy().into_owned(),
            ..Default::default()
        };

        let status = start_save_watcher_for_game(None, &entry).unwrap();
        assert_eq!(status.active_game_id.as_deref(), Some("g_resolve_1"));
        assert_eq!(status.debounce_ms, DEFAULT_DEBOUNCE_MS);
        assert!(status.watched_dirs.len() >= 2);

        let roots = resolve_save_roots_for_game(&entry);
        let (h1, count1, bytes1) = compute_save_tree_blake3(&roots).unwrap();
        assert_eq!(count1, 2);
        assert_eq!(bytes1, b"PROFILE_DATA".len() as u64 + b"SLOT0_DATA".len() as u64);
        let (h2, _, _) = compute_save_tree_blake3(&[roots[0].clone(), roots[0].join(".")]).unwrap();
        assert!(!h1.is_empty());
        assert!(!h2.is_empty());

        // Exercise multi-root snapshot staging + CAS path resolution + default label.
        let cas_dir = resolve_cas_dir_for_game(&entry).unwrap();
        assert!(cas_dir.ends_with(CAS_DIR_NAME));
        let multi_snap = capture_dedup_save_snapshot("g_resolve_1", &roots, &cas_dir, None)
            .unwrap()
            .expect("multi-root snapshot should be created");
        assert_eq!(multi_snap.label, AUTO_WATCHER_LABEL);
        assert_eq!(multi_snap.file_count, 2);

        let unset_entry = crate::GameEntry {
            id: "g_unset_root".to_string(),
            title: "Unset Root Game".to_string(),
            ..Default::default()
        };
        let fallback_cas = resolve_cas_dir_for_game(&unset_entry).unwrap();
        assert!(fallback_cas.ends_with(Path::new("save_cas").join("g_unset_root").join(CAS_DIR_NAME)));

        stop_all_save_watchers();
        assert!(get_save_watcher_status().active_game_id.is_none());

        fs::remove_dir_all(&root).unwrap();
    }
}
