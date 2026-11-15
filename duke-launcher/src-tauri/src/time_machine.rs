use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use walkdir::WalkDir;

const MAX_SAVE_FILES_PER_CHECKPOINT: usize = 1024;
const MAX_SINGLE_SAVE_FILE_BYTES: u64 = 64 * 1024 * 1024;
const CAS_DIR: &str = ".duke-cas";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeMachineFileEntry {
    pub save_root_idx: usize,
    pub rel_path: String,
    pub blake3: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeMachineCheckpoint {
    pub id: String,
    pub branch: String,
    pub parent_id: Option<String>,
    pub label: String,
    pub trigger: String,
    pub timestamp_secs: u64,
    pub playtime_secs: u64,
    pub file_count: usize,
    pub total_bytes: u64,
    pub new_cas_bytes: u64,
    pub dedup_saved_bytes: u64,
    pub thumbnail_data_url: Option<String>,
    pub save_roots: Vec<String>,
    pub files: Vec<TimeMachineFileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeMachineBranch {
    pub name: String,
    pub created_at: u64,
    pub parent_checkpoint_id: Option<String>,
    pub head_checkpoint_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeMachineTimelineResponse {
    pub game_id: String,
    pub current_branch: String,
    pub discovered_save_roots: Vec<String>,
    pub total_cas_bytes: u64,
    pub total_logical_bytes: u64,
    pub dedup_ratio_pct: f64,
    pub branches: Vec<TimeMachineBranch>,
    pub checkpoints: Vec<TimeMachineCheckpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeMachineRestoreReport {
    pub game_id: String,
    pub checkpoint_id: String,
    pub branch: String,
    pub files_restored: usize,
    pub bytes_restored: u64,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct TimelineManifestFile {
    current_branch: String,
    branches: Vec<TimeMachineBranch>,
    checkpoints: Vec<TimeMachineCheckpoint>,
}

struct SaveWatcherControl {
    stop_tx: std::sync::mpsc::Sender<()>,
    #[cfg(target_os = "windows")]
    stop_event_raw: usize,
}

static SAVE_WATCHERS: OnceLock<Mutex<HashMap<String, SaveWatcherControl>>> = OnceLock::new();
static TIMELINE_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn save_watchers_map() -> &'static Mutex<HashMap<String, SaveWatcherControl>> {
    SAVE_WATCHERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn timeline_lock() -> &'static Mutex<()> {
    TIMELINE_IO_LOCK.get_or_init(|| Mutex::new(()))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn sanitize_branch_name(raw: &str) -> Result<String, String> {
    let mut cleaned: String = raw
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else if c.is_whitespace() || c == '/' {
                '-'
            } else {
                '_'
            }
        })
        .collect();
    while cleaned.contains("--") {
        cleaned = cleaned.replace("--", "-");
    }
    let cleaned = cleaned.trim_matches(|c| c == '-' || c == '_').to_string();
    if cleaned.is_empty() {
        return Err("Branch name cannot be empty.".to_string());
    }
    Ok(cleaned.chars().take(48).collect())
}

fn game_store_dir(game_id: &str) -> Result<PathBuf, String> {
    let base = crate::get_app_data_file_standalone("time_machine")?;
    let safe_gid = crate::sanitize_filename(game_id);
    let gid = if safe_gid.is_empty() {
        blake3::hash(game_id.as_bytes()).to_hex()[..16].to_string()
    } else {
        safe_gid
    };
    let dir = base.join(gid);
    fs::create_dir_all(&dir).map_err(|e| format!("Failed creating Time-Machine directory: {e}"))?;
    Ok(dir)
}

fn load_manifest(store_dir: &Path) -> TimelineManifestFile {
    let path = store_dir.join("timeline.json");
    let mut file: TimelineManifestFile = fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    if file.current_branch.trim().is_empty() {
        file.current_branch = "main".to_string();
    }
    if !file.branches.iter().any(|b| b.name == "main") {
        file.branches.insert(
            0,
            TimeMachineBranch {
                name: "main".to_string(),
                created_at: now_secs(),
                parent_checkpoint_id: None,
                head_checkpoint_id: None,
            },
        );
    }
    file
}

fn save_manifest(store_dir: &Path, manifest: &TimelineManifestFile) -> Result<(), String> {
    fs::create_dir_all(store_dir)
        .map_err(|e| format!("Failed creating Time-Machine dir: {e}"))?;
    let json = serde_json::to_string_pretty(manifest)
        .map_err(|e| format!("Failed serializing timeline.json: {e}"))?;
    crate::atomic_write_string(&store_dir.join("timeline.json"), &json)
}

fn cas_blob_path(store_dir: &Path, digest: &str) -> PathBuf {
    let prefix = if digest.len() >= 2 { &digest[..2] } else { "00" };
    store_dir.join(CAS_DIR).join(prefix).join(format!("{digest}.zst"))
}

fn write_cas_blob_if_missing(store_dir: &Path, data: &[u8]) -> Result<(String, bool), String> {
    let digest = blake3::hash(data).to_hex().to_string();
    let blob_path = cas_blob_path(store_dir, &digest);
    if blob_path.is_file() {
        return Ok((digest, false));
    }
    if let Some(parent) = blob_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed creating .duke-cas shard: {e}"))?;
    }
    let compressed =
        zstd::encode_all(data, 3).map_err(|e| format!("Zstd CAS compression failed: {e}"))?;
    let tmp = blob_path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp).map_err(|e| format!("Failed creating CAS tmp: {e}"))?;
        f.write_all(&compressed)
            .map_err(|e| format!("Failed writing CAS blob: {e}"))?;
    }
    fs::rename(&tmp, &blob_path).map_err(|e| format!("Failed committing CAS blob: {e}"))?;
    Ok((digest, true))
}

fn read_cas_blob_verified(store_dir: &Path, digest: &str) -> Result<Vec<u8>, String> {
    let blob_path = cas_blob_path(store_dir, digest);
    let mut compressed = Vec::new();
    fs::File::open(&blob_path)
        .map_err(|e| format!("Missing .duke-cas chunk '{}': {e}", digest))?
        .read_to_end(&mut compressed)
        .map_err(|e| format!("Failed reading CAS chunk: {e}"))?;
    let raw =
        zstd::decode_all(Cursor::new(&compressed)).map_err(|e| format!("Zstd CAS decode error: {e}"))?;
    let actual = blake3::hash(&raw).to_hex().to_string();
    if actual != digest {
        return Err(format!(
            "BLAKE3 integrity mismatch on .duke-cas blob (expected {}, got {})",
            digest, actual
        ));
    }
    Ok(raw)
}

fn is_ignored_non_save_file(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "exe" | "dll" | "pak" | "iso" | "rvz" | "nsp" | "xci" | "chd" | "wua" | "vpk" | "bin"
    )
}

#[cfg(target_os = "windows")]
fn capture_160x90_thumbnail_data_url() -> Option<String> {
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        SRCCOPY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

    let (sw, sh, bgra) = unsafe {
        let sw = GetSystemMetrics(SM_CXSCREEN).max(1) as u32;
        let sh = GetSystemMetrics(SM_CYSCREEN).max(1) as u32;
        let hdc_screen = GetDC(None);
        if hdc_screen.is_invalid() {
            return None;
        }
        let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
        let hbm = CreateCompatibleBitmap(hdc_screen, sw as i32, sh as i32);
        let old = SelectObject(hdc_mem, hbm.into());
        let _ = BitBlt(
            hdc_mem,
            0,
            0,
            sw as i32,
            sh as i32,
            Some(hdc_screen),
            0,
            0,
            SRCCOPY,
        );
        let mut bi = BITMAPINFO::default();
        bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = sw as i32;
        bi.bmiHeader.biHeight = -(sh as i32);
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB.0;
        let mut pixels = vec![0u8; (sw * sh * 4) as usize];
        let scan = GetDIBits(
            hdc_mem,
            hbm,
            0,
            sh,
            Some(pixels.as_mut_ptr() as *mut std::ffi::c_void),
            &mut bi,
            DIB_RGB_COLORS,
        );
        let _ = SelectObject(hdc_mem, old);
        let _ = DeleteObject(hbm.into());
        let _ = DeleteDC(hdc_mem);
        let _ = ReleaseDC(None, hdc_screen);
        if scan == 0 {
            return None;
        }
        (sw, sh, pixels)
    };

    let mut rgb = Vec::with_capacity((sw * sh * 3) as usize);
    for px in bgra.as_chunks::<4>().0 {
        rgb.push(px[2]);
        rgb.push(px[1]);
        rgb.push(px[0]);
    }
    let img = image::RgbImage::from_raw(sw, sh, rgb)?;
    let resized =
        image::DynamicImage::ImageRgb8(img).resize_exact(160, 90, image::imageops::FilterType::Triangle);
    let mut jpeg_bytes = Vec::with_capacity(4096);
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(Cursor::new(&mut jpeg_bytes), 68);
    encoder.encode_image(&resized).ok()?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&jpeg_bytes);
    Some(format!("data:image/jpeg;base64,{}", b64))
}

#[cfg(not(target_os = "windows"))]
fn capture_160x90_thumbnail_data_url() -> Option<String> {
    None
}

fn dir_size_bytes(path: &Path) -> u64 {
    if !path.is_dir() {
        return 0;
    }
    WalkDir::new(path)
        .into_iter()
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

fn build_timeline_response(
    game_id: &str,
    store_dir: &Path,
    discovered_save_roots: Vec<String>,
) -> TimeMachineTimelineResponse {
    let manifest = load_manifest(store_dir);
    let total_cas_bytes = dir_size_bytes(&store_dir.join(CAS_DIR));
    let total_logical_bytes: u64 = manifest.checkpoints.iter().map(|c| c.total_bytes).sum();
    let dedup_ratio_pct = if total_logical_bytes > 0 {
        ((total_logical_bytes.saturating_sub(total_cas_bytes)) as f64 / total_logical_bytes as f64)
            * 100.0
    } else {
        0.0
    };
    let mut checkpoints = manifest.checkpoints;
    checkpoints.sort_by_key(|c| std::cmp::Reverse(c.timestamp_secs));

    TimeMachineTimelineResponse {
        game_id: game_id.to_string(),
        current_branch: manifest.current_branch,
        discovered_save_roots,
        total_cas_bytes,
        total_logical_bytes,
        dedup_ratio_pct,
        branches: manifest.branches,
        checkpoints,
    }
}

pub(crate) fn create_checkpoint_in_store(
    store_dir: &Path,
    save_roots: &[PathBuf],
    label: &str,
    trigger: &str,
    playtime_secs: u64,
    thumbnail_data_url: Option<String>,
) -> Result<TimeMachineCheckpoint, String> {
    let _guard = timeline_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut manifest = load_manifest(store_dir);
    let branch_name = manifest.current_branch.clone();
    let parent_id = manifest
        .branches
        .iter()
        .find(|b| b.name == branch_name)
        .and_then(|b| b.head_checkpoint_id.clone());

    let mut files = Vec::new();
    let mut total_bytes = 0u64;
    let mut new_cas_bytes = 0u64;
    let mut dedup_saved_bytes = 0u64;
    let mut root_strings = Vec::with_capacity(save_roots.len());

    for (root_idx, root) in save_roots.iter().enumerate() {
        root_strings.push(root.to_string_lossy().into_owned());
        if root.is_file() {
            if let Ok(bytes) = fs::read(root) {
                let sz = bytes.len() as u64;
                if sz <= MAX_SINGLE_SAVE_FILE_BYTES {
                    let (digest, newly_stored) = write_cas_blob_if_missing(store_dir, &bytes)?;
                    total_bytes = total_bytes.saturating_add(sz);
                    if newly_stored {
                        new_cas_bytes = new_cas_bytes.saturating_add(sz);
                    } else {
                        dedup_saved_bytes = dedup_saved_bytes.saturating_add(sz);
                    }
                    let rel_name = root
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("save.dat")
                        .to_string();
                    files.push(TimeMachineFileEntry {
                        save_root_idx: root_idx,
                        rel_path: rel_name,
                        blake3: digest,
                        size_bytes: sz,
                    });
                }
            }
            continue;
        }
        if !root.is_dir() {
            continue;
        }
        for entry in WalkDir::new(root)
            .max_depth(6)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
        {
            if files.len() >= MAX_SAVE_FILES_PER_CHECKPOINT {
                break;
            }
            let path = entry.path();
            if is_ignored_non_save_file(path) {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.len() > MAX_SINGLE_SAVE_FILE_BYTES {
                continue;
            }
            let Ok(rel) = path.strip_prefix(root) else {
                continue;
            };
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if rel_str.contains(".duke-cas") {
                continue;
            }
            let Ok(bytes) = fs::read(path) else {
                continue;
            };
            let sz = bytes.len() as u64;
            let (digest, newly_stored) = write_cas_blob_if_missing(store_dir, &bytes)?;
            total_bytes = total_bytes.saturating_add(sz);
            if newly_stored {
                new_cas_bytes = new_cas_bytes.saturating_add(sz);
            } else {
                dedup_saved_bytes = dedup_saved_bytes.saturating_add(sz);
            }
            files.push(TimeMachineFileEntry {
                save_root_idx: root_idx,
                rel_path: rel_str,
                blake3: digest,
                size_bytes: sz,
            });
        }
    }

    if files.is_empty() {
        return Err("No save files found inside discovered save directories yet.".to_string());
    }

    // Skip creating duplicate back-to-back auto checkpoints on the exact same save digest set.
    if trigger == "auto-watch" {
        if let Some(head_id) = &parent_id {
            if let Some(prev) = manifest.checkpoints.iter().find(|c| &c.id == head_id) {
                let prev_sigs: Vec<(&str, &str)> = prev
                    .files
                    .iter()
                    .map(|f| (f.rel_path.as_str(), f.blake3.as_str()))
                    .collect();
                let cur_sigs: Vec<(&str, &str)> = files
                    .iter()
                    .map(|f| (f.rel_path.as_str(), f.blake3.as_str()))
                    .collect();
                if prev_sigs == cur_sigs {
                    return Ok(prev.clone());
                }
            }
        }
    }

    let ts = now_secs();
    let seq = manifest.checkpoints.len() + 1;
    let id = format!("tm-{}-{}-{}", branch_name, ts, seq);
    let checkpoint = TimeMachineCheckpoint {
        id: id.clone(),
        branch: branch_name.clone(),
        parent_id,
        label: if label.trim().is_empty() {
            format!("Checkpoint #{}", seq)
        } else {
            label.trim().to_string()
        },
        trigger: trigger.to_string(),
        timestamp_secs: ts,
        playtime_secs,
        file_count: files.len(),
        total_bytes,
        new_cas_bytes,
        dedup_saved_bytes,
        thumbnail_data_url,
        save_roots: root_strings,
        files,
    };

    if let Some(br) = manifest.branches.iter_mut().find(|b| b.name == branch_name) {
        br.head_checkpoint_id = Some(id);
    }
    manifest.checkpoints.push(checkpoint.clone());
    save_manifest(store_dir, &manifest)?;
    Ok(checkpoint)
}

pub(crate) fn create_branch_in_store(
    store_dir: &Path,
    branch_name: &str,
    from_checkpoint_id: Option<&str>,
) -> Result<TimelineManifestFile, String> {
    let _guard = timeline_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let clean = sanitize_branch_name(branch_name)?;
    let mut manifest = load_manifest(store_dir);
    if manifest.branches.iter().any(|b| b.name == clean) {
        return Err(format!("Branch '{}' already exists.", clean));
    }
    let parent_cp = from_checkpoint_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            manifest
                .branches
                .iter()
                .find(|b| b.name == manifest.current_branch)
                .and_then(|b| b.head_checkpoint_id.clone())
        });

    manifest.branches.push(TimeMachineBranch {
        name: clean.clone(),
        created_at: now_secs(),
        parent_checkpoint_id: parent_cp.clone(),
        head_checkpoint_id: parent_cp,
    });
    manifest.current_branch = clean;
    save_manifest(store_dir, &manifest)?;
    Ok(manifest)
}

pub(crate) fn restore_checkpoint_in_store(
    store_dir: &Path,
    checkpoint_id: &str,
) -> Result<(TimeMachineCheckpoint, usize, u64), String> {
    let manifest = {
        let _guard = timeline_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        load_manifest(store_dir)
    };
    let cp = manifest
        .checkpoints
        .iter()
        .find(|c| c.id == checkpoint_id)
        .ok_or_else(|| format!("Time-Machine checkpoint '{}' not found.", checkpoint_id))?
        .clone();

    let mut restored_files = 0usize;
    let mut restored_bytes = 0u64;
    for item in &cp.files {
        let Some(root_str) = cp.save_roots.get(item.save_root_idx) else {
            continue;
        };
        let root_path = Path::new(root_str);
        let rel_clean = item.rel_path.trim_start_matches('/');
        if rel_clean.contains("..") {
            continue;
        }
        let target = if root_path.is_file() {
            root_path.to_path_buf()
        } else {
            root_path.join(rel_clean.replace('/', std::path::MAIN_SEPARATOR_STR))
        };
        let bytes = read_cas_blob_verified(store_dir, &item.blake3)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed creating save restore dir: {e}"))?;
        }
        fs::write(&target, &bytes)
            .map_err(|e| format!("Failed restoring save file {}: {e}", target.display()))?;
        restored_files += 1;
        restored_bytes = restored_bytes.saturating_add(bytes.len() as u64);
    }

    Ok((cp, restored_files, restored_bytes))
}

async fn resolve_save_roots_for_game(game_id: &str) -> (Vec<PathBuf>, u64) {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    if let Ok(report) = crate::ludusavi::discover_game_save_paths_core(game_id, false).await {
        for cand in report.existing_paths {
            let p = PathBuf::from(&cand.path);
            let key = cand.path.to_ascii_lowercase();
            if p.exists() && seen.insert(key) {
                roots.push(p);
            }
        }
    }

    let gid_owned = game_id.to_string();
    let (extra_roots, playtime) = tokio::task::spawn_blocking(move || {
        let mut out = Vec::new();
        let mut play = 0u64;
        if let Ok(lib) = crate::load_library_standalone() {
            if let Some(entry) = lib
                .iter()
                .find(|g| g.id == gid_owned || g.exe_path.eq_ignore_ascii_case(&gid_owned))
            {
                play = entry.total_playtime_secs();
                if !entry.install_dir.trim().is_empty() {
                    let gdir = PathBuf::from(entry.install_dir.trim());
                    for sub in ["Saves", "saves", "save", "SaveData", "savedata"] {
                        let cand = gdir.join(sub);
                        if cand.is_dir() {
                            out.push(cand);
                        }
                    }
                }
            }
        }
        (out, play)
    })
    .await
    .unwrap_or_default();

    for p in extra_roots {
        let key = p.to_string_lossy().to_ascii_lowercase();
        if seen.insert(key) {
            roots.push(p);
        }
    }
    (roots, playtime)
}

#[tauri::command]
pub(crate) async fn get_game_time_machine(
    game_id: String,
) -> Result<TimeMachineTimelineResponse, String> {
    let (roots, _) = resolve_save_roots_for_game(&game_id).await;
    let store_dir = game_store_dir(&game_id)?;
    let root_strs: Vec<String> = roots
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    Ok(build_timeline_response(&game_id, &store_dir, root_strs))
}

#[tauri::command]
pub(crate) async fn create_time_machine_checkpoint(
    app: AppHandle,
    game_id: String,
    label: Option<String>,
    trigger: Option<String>,
) -> Result<TimeMachineCheckpoint, String> {
    let (roots, playtime) = resolve_save_roots_for_game(&game_id).await;
    let store_dir = game_store_dir(&game_id)?;
    let thumb = tokio::task::spawn_blocking(capture_160x90_thumbnail_data_url)
        .await
        .ok()
        .flatten();
    let label_str = label.unwrap_or_else(|| "Manual Snapshot".to_string());
    let trigger_str = trigger.unwrap_or_else(|| "manual".to_string());
    let cp = tokio::task::spawn_blocking(move || {
        create_checkpoint_in_store(&store_dir, &roots, &label_str, &trigger_str, playtime, thumb)
    })
    .await
    .map_err(|e| format!("Checkpoint task failed: {e}"))??;
    let _ = app.emit("time-machine-updated", &game_id);
    Ok(cp)
}

#[tauri::command]
pub(crate) async fn create_time_machine_branch(
    game_id: String,
    branch_name: String,
    from_checkpoint_id: Option<String>,
) -> Result<TimeMachineTimelineResponse, String> {
    let (roots, _) = resolve_save_roots_for_game(&game_id).await;
    let store_dir = game_store_dir(&game_id)?;
    create_branch_in_store(&store_dir, &branch_name, from_checkpoint_id.as_deref())?;
    let root_strs: Vec<String> = roots
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    Ok(build_timeline_response(&game_id, &store_dir, root_strs))
}

#[tauri::command]
pub(crate) async fn switch_time_machine_branch(
    app: AppHandle,
    game_id: String,
    branch_name: String,
    restore_head: Option<bool>,
) -> Result<TimeMachineTimelineResponse, String> {
    let clean = sanitize_branch_name(&branch_name)?;
    let store_dir = game_store_dir(&game_id)?;
    let head_id = {
        let _guard = timeline_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut manifest = load_manifest(&store_dir);
        let head = manifest
            .branches
            .iter()
            .find(|b| b.name == clean)
            .ok_or_else(|| format!("Branch '{}' does not exist.", clean))?
            .head_checkpoint_id
            .clone();
        manifest.current_branch = clean.clone();
        save_manifest(&store_dir, &manifest)?;
        head
    };

    if restore_head.unwrap_or(true) {
        if let Some(cp_id) = head_id {
            let store_clone = store_dir.clone();
            let _ = tokio::task::spawn_blocking(move || {
                restore_checkpoint_in_store(&store_clone, &cp_id)
            })
            .await;
        }
    }

    let (roots, _) = resolve_save_roots_for_game(&game_id).await;
    let root_strs: Vec<String> = roots
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let _ = app.emit("time-machine-updated", &game_id);
    Ok(build_timeline_response(&game_id, &store_dir, root_strs))
}

#[tauri::command]
pub(crate) async fn restore_time_machine_checkpoint(
    app: AppHandle,
    game_id: String,
    checkpoint_id: String,
) -> Result<TimeMachineRestoreReport, String> {
    let store_dir = game_store_dir(&game_id)?;
    let cp_id = checkpoint_id.clone();
    let (cp, files_restored, bytes_restored) = tokio::task::spawn_blocking(move || {
        restore_checkpoint_in_store(&store_dir, &cp_id)
    })
    .await
    .map_err(|e| format!("Restore task failed: {e}"))??;

    let report = TimeMachineRestoreReport {
        game_id: game_id.clone(),
        checkpoint_id: cp.id.clone(),
        branch: cp.branch.clone(),
        files_restored,
        bytes_restored,
        summary: format!(
            "Restored '{}' ({} files, {} KB) on branch '{}'",
            cp.label,
            files_restored,
            bytes_restored / 1024,
            cp.branch
        ),
    };
    let _ = app.emit("time-machine-updated", &game_id);
    Ok(report)
}

#[tauri::command]
pub(crate) async fn delete_time_machine_checkpoint(
    game_id: String,
    checkpoint_id: String,
) -> Result<TimeMachineTimelineResponse, String> {
    let store_dir = game_store_dir(&game_id)?;
    {
        let _guard = timeline_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut manifest = load_manifest(&store_dir);
        manifest.checkpoints.retain(|c| c.id != checkpoint_id);
        for br in &mut manifest.branches {
            if br.head_checkpoint_id.as_deref() == Some(checkpoint_id.as_str()) {
                br.head_checkpoint_id = manifest
                    .checkpoints
                    .iter()
                    .rev()
                    .find(|c| c.branch == br.name)
                    .map(|c| c.id.clone());
            }
        }
        save_manifest(&store_dir, &manifest)?;
    }
    let (roots, _) = resolve_save_roots_for_game(&game_id).await;
    let root_strs: Vec<String> = roots
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    Ok(build_timeline_response(&game_id, &store_dir, root_strs))
}

pub(crate) fn on_achievement_checkpoint(app: &AppHandle, game_id: &str, achievement_name: &str) {
    let app_clone = app.clone();
    let gid = game_id.to_string();
    let label = format!("Achievement: {}", achievement_name);
    tauri::async_runtime::spawn(async move {
        let _ = create_time_machine_checkpoint(
            app_clone,
            gid,
            Some(label),
            Some("achievement".to_string()),
        )
        .await;
    });
}

pub(crate) fn start_save_watcher(app: AppHandle, game: &crate::GameEntry) {
    stop_save_watcher(&game.id);
    let gid = game.id.clone();
    let playtime = game.total_playtime_secs();

    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    #[cfg(target_os = "windows")]
    let stop_event_raw: usize = unsafe {
        use windows::Win32::System::Threading::CreateEventW;
        CreateEventW(None, true, false, None)
            .map(|h| h.0 as usize)
            .unwrap_or(0)
    };

    {
        let mut map = save_watchers_map()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        map.insert(
            gid.clone(),
            SaveWatcherControl {
                stop_tx,
                #[cfg(target_os = "windows")]
                stop_event_raw,
            },
        );
    }

    std::thread::Builder::new()
        .name(format!("duke-tm-watch-{}", gid))
        .spawn(move || {
            let roots = tauri::async_runtime::block_on(async {
                resolve_save_roots_for_game(&gid).await.0
            });
            let dir_roots: Vec<PathBuf> = roots.iter().filter(|p| p.is_dir()).cloned().collect();
            let Ok(store_dir) = game_store_dir(&gid) else {
                return;
            };
            #[cfg(target_os = "windows")]
            {
                run_win32_save_watcher(SaveWatcherRunCfg {
                    app: &app,
                    game_id: &gid,
                    store_dir: &store_dir,
                    all_roots: &roots,
                    watch_dirs: &dir_roots,
                    playtime_secs: playtime,
                    stop_event_raw,
                    stop_rx,
                });
                if stop_event_raw != 0 {
                    unsafe {
                        use windows::Win32::Foundation::{CloseHandle, HANDLE};
                        let _ = CloseHandle(HANDLE(stop_event_raw as *mut std::ffi::c_void));
                    }
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = (app, store_dir, roots, dir_roots, playtime, stop_rx);
            }
        })
        .ok();
}

pub(crate) fn stop_save_watcher(game_id: &str) {
    let ctrl = save_watchers_map()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(game_id);
    if let Some(c) = ctrl {
        let _ = c.stop_tx.send(());
        #[cfg(target_os = "windows")]
        if c.stop_event_raw != 0 {
            unsafe {
                use windows::Win32::Foundation::HANDLE;
                use windows::Win32::System::Threading::SetEvent;
                let _ = SetEvent(HANDLE(c.stop_event_raw as *mut std::ffi::c_void));
            }
        }
    }
}

#[cfg(target_os = "windows")]
struct SaveWatcherRunCfg<'a> {
    app: &'a AppHandle,
    game_id: &'a str,
    store_dir: &'a Path,
    all_roots: &'a [PathBuf],
    watch_dirs: &'a [PathBuf],
    playtime_secs: u64,
    stop_event_raw: usize,
    stop_rx: std::sync::mpsc::Receiver<()>,
}

#[cfg(target_os = "windows")]
fn run_win32_save_watcher(cfg: SaveWatcherRunCfg<'_>) {
    let SaveWatcherRunCfg {
        app,
        game_id,
        store_dir,
        all_roots,
        watch_dirs,
        playtime_secs,
        stop_event_raw,
        stop_rx,
    } = cfg;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, ReadDirectoryChangesW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED,
        FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
    use windows::Win32::System::Threading::{CreateEventW, ResetEvent, WaitForMultipleObjects};

    if watch_dirs.is_empty() || stop_event_raw == 0 {
        while stop_rx.recv_timeout(Duration::from_secs(10)).is_err() {}
        return;
    }

    struct SaveDirWatch {
        dir_handle: HANDLE,
        io_event: HANDLE,
        overlapped: Box<OVERLAPPED>,
        buffer: Vec<u8>,
    }

    let mut watches = Vec::new();
    let filter = FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE;

    for dir in watch_dirs.iter().take(8) {
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
        let mut buffer = vec![0u8; 8192];
        let ok = unsafe {
            ReadDirectoryChangesW(
                dh,
                buffer.as_mut_ptr() as *mut std::ffi::c_void,
                buffer.len() as u32,
                true,
                filter,
                None,
                Some(&mut *overlapped),
                None,
            )
            .is_ok()
        };
        if ok {
            watches.push(SaveDirWatch {
                dir_handle: dh,
                io_event: ev,
                overlapped,
                buffer,
            });
        } else {
            unsafe {
                let _ = CloseHandle(ev);
                let _ = CloseHandle(dh);
            }
        }
    }

    let stop_handle = HANDLE(stop_event_raw as *mut std::ffi::c_void);
    let mut last_snapshot = Instant::now() - Duration::from_secs(30);

    loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }
        let mut wait_handles = Vec::with_capacity(watches.len() + 1);
        wait_handles.push(stop_handle);
        for w in &watches {
            wait_handles.push(w.io_event);
        }
        let res = unsafe { WaitForMultipleObjects(&wait_handles, false, 4000) };
        if res.0 == WAIT_OBJECT_0.0 {
            break;
        }
        let idx = res.0.wrapping_sub(WAIT_OBJECT_0.0 + 1) as usize;
        if idx < watches.len() {
            let item = &mut watches[idx];
            unsafe {
                let _ = ResetEvent(item.io_event);
                let _ = ReadDirectoryChangesW(
                    item.dir_handle,
                    item.buffer.as_mut_ptr() as *mut std::ffi::c_void,
                    item.buffer.len() as u32,
                    true,
                    filter,
                    None,
                    Some(&mut *item.overlapped),
                    None,
                );
            }
            if last_snapshot.elapsed() >= Duration::from_millis(2500) {
                std::thread::sleep(Duration::from_millis(200));
                last_snapshot = Instant::now();
                let thumb = capture_160x90_thumbnail_data_url();
                if create_checkpoint_in_store(
                    store_dir,
                    all_roots,
                    "Auto Save Checkpoint",
                    "auto-watch",
                    playtime_secs,
                    thumb,
                )
                .is_ok()
                {
                    let _ = app.emit("time-machine-updated", game_id);
                }
            }
        }
    }

    for item in &mut watches {
        unsafe {
            let _ = CancelIoEx(item.dir_handle, Some(&*item.overlapped));
            let mut drained = 0u32;
            let _ = GetOverlappedResult(item.dir_handle, &*item.overlapped, &mut drained, true);
            let _ = CloseHandle(item.io_event);
            let _ = CloseHandle(item.dir_handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_machine_branch_dedup_and_rollback_roundtrip() {
        let root = std::env::temp_dir().join(format!("duke_tm_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let save_dir = root.join("saves");
        let store_dir = root.join("store");
        fs::create_dir_all(&save_dir).unwrap();

        let shared_assets = vec![0x5Au8; 8192];
        fs::write(save_dir.join("slot1.sav"), b"LEVEL_01_MAIN_HP=100").unwrap();
        fs::write(save_dir.join("world_map.dat"), &shared_assets).unwrap();

        let cp1 = create_checkpoint_in_store(
            &store_dir,
            std::slice::from_ref(&save_dir),
            "Before Boss Fork",
            "manual",
            120,
            None,
        )
        .unwrap();
        assert_eq!(cp1.branch, "main");
        assert_eq!(cp1.file_count, 2);
        assert!(cp1.new_cas_bytes > 8192);
        assert_eq!(cp1.dedup_saved_bytes, 0);

        create_branch_in_store(&store_dir, "Boss Route B", Some(&cp1.id)).unwrap();
        fs::write(save_dir.join("slot1.sav"), b"LEVEL_02_BRANCH_B_HP=40").unwrap();

        let cp2 = create_checkpoint_in_store(
            &store_dir,
            std::slice::from_ref(&save_dir),
            "Boss Route B Mid",
            "manual",
            240,
            None,
        )
        .unwrap();
        assert_eq!(cp2.branch, "boss-route-b");
        assert_eq!(cp2.dedup_saved_bytes, 8192);
        assert_eq!(cp2.new_cas_bytes, b"LEVEL_02_BRANCH_B_HP=40".len() as u64);

        let (restored_cp, files_n, _bytes_n) =
            restore_checkpoint_in_store(&store_dir, &cp1.id).unwrap();
        assert_eq!(restored_cp.id, cp1.id);
        assert_eq!(files_n, 2);
        assert_eq!(
            fs::read(save_dir.join("slot1.sav")).unwrap(),
            b"LEVEL_01_MAIN_HP=100"
        );
        assert_eq!(
            fs::read(save_dir.join("world_map.dat")).unwrap(),
            shared_assets
        );

        let _ = fs::remove_dir_all(&root);
    }
}
