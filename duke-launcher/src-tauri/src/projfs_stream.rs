use crate::delta_patch::{FASTCDC_AVG_SIZE, FASTCDC_MAX_SIZE, FASTCDC_MIN_SIZE};
use crate::snapshots::CAS_DIR_NAME;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use tauri::Emitter;
use walkdir::WalkDir;

#[cfg(target_os = "windows")]
use std::ffi::c_void;
#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
#[cfg(target_os = "windows")]
use windows::core::{GUID, PCWSTR};
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HMODULE;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

pub const CORE_EARLY_PLAY_PERCENT: f64 = 10.0;
const CAS_BLOCK_EXT: &str = "dkblk";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjFsRuntimeStatus {
    pub dll_present: bool,
    pub api_bound: bool,
    pub feature_enabled_hint: bool,
    pub cas_root_dir: String,
    pub active_mounts: Vec<ProjFsMountInfo>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjFsMountInfo {
    pub mount_id: String,
    pub game_title: String,
    pub mount_dir: String,
    pub backing_url: String,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub ready_to_play_early: bool,
    pub on_demand_reads: u64,
    pub cas_cache_hits: u64,
    pub remote_prioritized_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualFileSpan {
    pub rel_path: String,
    pub file_size: u64,
    /// Offset in the backing linear archive or HTTP download stream.
    pub stream_offset: u64,
    pub chunks: Vec<VirtualChunkRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualChunkRef {
    pub file_offset: u64,
    pub stream_offset: u64,
    pub length: u32,
    pub blake3: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriorityByteRange {
    pub start: u64,
    pub end: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CasDedupReport {
    pub cas_root: String,
    pub scanned_games: usize,
    pub scanned_files: usize,
    pub unique_blocks: usize,
    pub total_logical_bytes: u64,
    pub deduplicated_bytes: u64,
    pub hardlinked_files: usize,
    pub summary: String,
}

/// Coordinates on-demand ProjFS byte-range requests with the 8-worker AIMD HTTP downloader.
pub struct ProjFsPriorityCoordinator {
    total_bytes: u64,
    urgent_queue: Mutex<VecDeque<PriorityByteRange>>,
    completed_ranges: Mutex<Vec<(u64, u64)>>,
    ready_signal: Condvar,
    core_threshold_reached: AtomicBool,
    on_demand_reads: AtomicU64,
    cas_cache_hits: AtomicU64,
    remote_prioritized_bytes: AtomicU64,
}

impl ProjFsPriorityCoordinator {
    pub fn new(_job_id: &str, total_bytes: u64) -> Arc<Self> {
        Arc::new(Self {
            total_bytes,
            urgent_queue: Mutex::new(VecDeque::new()),
            completed_ranges: Mutex::new(Vec::new()),
            ready_signal: Condvar::new(),
            core_threshold_reached: AtomicBool::new(false),
            on_demand_reads: AtomicU64::new(0),
            cas_cache_hits: AtomicU64::new(0),
            remote_prioritized_bytes: AtomicU64::new(0),
        })
    }

    pub fn enqueue_urgent_range(&self, start: u64, end_inclusive: u64) {
        if self.total_bytes > 0 && start >= self.total_bytes {
            return;
        }
        let clamped_end = if self.total_bytes > 0 {
            end_inclusive.min(self.total_bytes.saturating_sub(1))
        } else {
            end_inclusive
        };
        if start > clamped_end || self.is_range_completed(start, clamped_end) {
            return;
        }
        if let Ok(mut q) = self.urgent_queue.lock() {
            // Coalesce overlapping/adjacent range requests into merged spans.
            let mut merged_start = start;
            let mut merged_end = clamped_end;
            let mut kept = VecDeque::with_capacity(q.len() + 1);
            while let Some(existing) = q.pop_front() {
                if existing.end.saturating_add(1) >= merged_start
                    && existing.start <= merged_end.saturating_add(1)
                {
                    merged_start = merged_start.min(existing.start);
                    merged_end = merged_end.max(existing.end);
                } else {
                    kept.push_back(existing);
                }
            }
            kept.push_front(PriorityByteRange {
                start: merged_start,
                end: merged_end,
            });
            *q = kept;
        }
    }

    pub fn pop_urgent_range(&self) -> Option<PriorityByteRange> {
        self.urgent_queue
            .lock()
            .ok()
            .and_then(|mut q| q.pop_front())
    }

    pub fn mark_range_completed(&self, start: u64, end_inclusive: u64, prioritized: bool) {
        if start > end_inclusive {
            return;
        }
        if prioritized {
            self.remote_prioritized_bytes
                .fetch_add(end_inclusive - start + 1, Ordering::Relaxed);
        }
        if let Ok(mut guard) = self.completed_ranges.lock() {
            guard.push((start, end_inclusive));
            merge_intervals_in_place(&mut guard);
        }
        self.ready_signal.notify_all();
    }

    pub fn is_range_completed(&self, start: u64, end_inclusive: u64) -> bool {
        if start > end_inclusive {
            return true;
        }
        let Ok(guard) = self.completed_ranges.lock() else {
            return false;
        };
        guard
            .iter()
            .any(|&(s, e)| s <= start && e >= end_inclusive)
    }

    pub fn wait_for_range(&self, start: u64, end_inclusive: u64, timeout: std::time::Duration) -> bool {
        let Ok(mut guard) = self.completed_ranges.lock() else {
            return false;
        };
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if guard.iter().any(|&(s, e)| s <= start && e >= end_inclusive) {
                return true;
            }
            let Some(rem) = deadline.checked_duration_since(std::time::Instant::now()) else {
                return false;
            };
            let (next_guard, wait_res) = self.ready_signal.wait_timeout(guard, rem).unwrap();
            guard = next_guard;
            if wait_res.timed_out() {
                return guard.iter().any(|&(s, e)| s <= start && e >= end_inclusive);
            }
        }
    }

    pub fn update_progress(&self, downloaded_bytes: u64) -> bool {
        if self.total_bytes == 0 {
            return false;
        }
        let pct = (downloaded_bytes as f64 / self.total_bytes as f64) * 100.0;
        if pct >= CORE_EARLY_PLAY_PERCENT
            && !self.core_threshold_reached.swap(true, Ordering::AcqRel)
        {
            return true;
        }
        false
    }

    pub fn record_cas_hit(&self) {
        self.cas_cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_on_demand_read(&self) {
        self.on_demand_reads.fetch_add(1, Ordering::Relaxed);
    }
}

fn merge_intervals_in_place(spans: &mut Vec<(u64, u64)>) {
    if spans.len() <= 1 {
        return;
    }
    spans.sort_unstable();
    let mut out: Vec<(u64, u64)> = Vec::with_capacity(spans.len());
    for &(s, e) in spans.iter() {
        if let Some(last) = out.last_mut() {
            if s <= last.1.saturating_add(1) {
                last.1 = last.1.max(e);
                continue;
            }
        }
        out.push((s, e));
    }
    *spans = out;
}

static JOB_COORDINATORS: OnceLock<Mutex<HashMap<String, Arc<ProjFsPriorityCoordinator>>>> =
    OnceLock::new();

fn job_coordinators() -> &'static Mutex<HashMap<String, Arc<ProjFsPriorityCoordinator>>> {
    JOB_COORDINATORS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn register_job_coordinator(job_id: &str, total_bytes: u64) -> Arc<ProjFsPriorityCoordinator> {
    let coord = ProjFsPriorityCoordinator::new(job_id, total_bytes);
    if let Ok(mut guard) = job_coordinators().lock() {
        guard.insert(job_id.to_string(), Arc::clone(&coord));
    }
    coord
}

pub fn unregister_job_coordinator(job_id: &str) {
    if let Ok(mut guard) = job_coordinators().lock() {
        guard.remove(job_id);
    }
}

pub fn get_job_coordinator(job_id: &str) -> Option<Arc<ProjFsPriorityCoordinator>> {
    job_coordinators()
        .lock()
        .ok()
        .and_then(|g| g.get(job_id).cloned())
}

pub fn shared_cas_root_dir() -> PathBuf {
    if let Ok(path) = crate::get_app_data_file_standalone(CAS_DIR_NAME) {
        return path;
    }
    std::env::temp_dir().join("duke-launcher-cas")
}

fn cas_block_path(cas_root: &Path, blake3_hex: &str) -> PathBuf {
    let clean: String = blake3_hex
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect();
    let prefix = clean.get(..2).unwrap_or("00");
    cas_root
        .join("blocks")
        .join(prefix)
        .join(format!("{clean}.{CAS_BLOCK_EXT}"))
}

pub fn store_cas_block(cas_root: &Path, data: &[u8]) -> Result<String, String> {
    let hash = blake3::hash(data).to_hex().to_string();
    let dst = cas_block_path(cas_root, &hash);
    if dst.is_file() {
        return Ok(hash);
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed creating CAS block folder {}: {e}", parent.display()))?;
    }
    let tmp = dst.with_extension("tmp");
    {
        let mut f = File::create(&tmp)
            .map_err(|e| format!("Failed writing CAS block {}: {e}", tmp.display()))?;
        f.write_all(data)
            .map_err(|e| format!("Failed flushing CAS block {}: {e}", tmp.display()))?;
    }
    if !dst.is_file() {
        let _ = fs::rename(&tmp, &dst);
    }
    let _ = fs::remove_file(&tmp);
    Ok(hash)
}

pub fn read_cas_block(cas_root: &Path, blake3_hex: &str) -> Option<Vec<u8>> {
    let path = cas_block_path(cas_root, blake3_hex);
    fs::read(path).ok()
}

/// Splits a file into FastCDC 16-64 KiB chunks, stores unique blocks in `.duke-cas`, and returns chunk refs.
#[cfg(test)]
pub fn index_file_into_cas(
    cas_root: &Path,
    file_path: &Path,
    stream_offset_base: u64,
) -> Result<(u64, Vec<VirtualChunkRef>), String> {
    let mut file = File::open(file_path)
        .map_err(|e| format!("Failed opening {} for FastCDC CAS: {e}", file_path.display()))?;
    let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut chunks = Vec::new();
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)
        .map_err(|e| format!("Failed reading {} for FastCDC CAS: {e}", file_path.display()))?;

    let cdc = fastcdc::v2020::FastCDC::new(
        &buf,
        FASTCDC_MIN_SIZE,
        FASTCDC_AVG_SIZE,
        FASTCDC_MAX_SIZE,
    );
    for ch in cdc {
        let slice = &buf[ch.offset..ch.offset + ch.length];
        let hash = store_cas_block(cas_root, slice)?;
        chunks.push(VirtualChunkRef {
            file_offset: ch.offset as u64,
            stream_offset: stream_offset_base.saturating_add(ch.offset as u64),
            length: ch.length as u32,
            blake3: hash,
        });
    }
    Ok((file_size, chunks))
}

/// Resolves which backing stream byte ranges cover a requested virtual file window `[offset, offset + len)`.
pub fn map_virtual_read_to_stream_ranges(
    file_span: &VirtualFileSpan,
    offset: u64,
    len: u32,
) -> Vec<(u64, u64, String)> {
    let req_end = offset.saturating_add(len as u64);
    if file_span.chunks.is_empty() {
        let clamped_end = req_end.min(file_span.file_size);
        if clamped_end <= offset {
            return Vec::new();
        }
        return vec![(
            file_span.stream_offset.saturating_add(offset),
            file_span
                .stream_offset
                .saturating_add(clamped_end)
                .saturating_sub(1),
            String::new(),
        )];
    }

    let mut out = Vec::new();
    for ch in &file_span.chunks {
        let ch_start = ch.file_offset;
        let ch_end = ch.file_offset.saturating_add(ch.length as u64);
        if ch_end <= offset || ch_start >= req_end {
            continue;
        }
        out.push((
            ch.stream_offset,
            ch.stream_offset
                .saturating_add(ch.length as u64)
                .saturating_sub(1),
            ch.blake3.clone(),
        ));
    }
    out
}

/// Checks if a file is a strong candidate for cross-game Unreal/Unity/runtime deduplication.
fn is_dedup_engine_asset(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(
        ext.as_str(),
        "pak" | "ucas" | "utoc" | "assets" | "ress" | "bundle" | "dll" | "bk2" | "wem"
    ) {
        return true;
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    name.starts_with("sharedassets")
        || name.starts_with("globalgamemanagers")
        || name.starts_with("unityplayer")
        || name.starts_with("physx")
        || name.starts_with("oo2core")
}

/// Scans game folders, deduplicates shared engine assets into `.duke-cas/` and hardlinks identical files.
pub fn deduplicate_game_dirs_cas(game_dirs: &[PathBuf], cas_root: &Path) -> Result<CasDedupReport, String> {
    fs::create_dir_all(cas_root.join("blocks"))
        .map_err(|e| format!("Failed creating CAS directory: {e}"))?;

    let mut whole_file_owners: HashMap<String, PathBuf> = HashMap::new();
    let mut unique_blocks: HashSet<String> = HashSet::new();
    let mut total_logical_bytes = 0u64;
    let mut deduplicated_bytes = 0u64;
    let mut hardlinked_files = 0usize;
    let mut scanned_files = 0usize;
    let mut scanned_games = 0usize;

    for dir in game_dirs {
        if !dir.is_dir() {
            continue;
        }
        scanned_games += 1;
        for entry in WalkDir::new(dir).follow_links(false).into_iter().flatten() {
            let path = entry.path();
            if !entry.file_type().is_file() || !is_dedup_engine_asset(path) {
                continue;
            }
            if path
                .components()
                .any(|c| c.as_os_str() == CAS_DIR_NAME || c.as_os_str() == ".duke-profile")
            {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            let len = meta.len();
            if len < 64 * 1024 {
                continue;
            }
            scanned_files += 1;
            total_logical_bytes = total_logical_bytes.saturating_add(len);

            let Ok(mut file) = File::open(path) else {
                continue;
            };
            let mut hasher = blake3::Hasher::new();
            let mut raw = Vec::new();
            let Ok(read_bytes) = file.read_to_end(&mut raw) else {
                continue;
            };
            if read_bytes == 0 {
                continue;
            }
            hasher.update(&raw);
            let file_hash = hasher.finalize().to_hex().to_string();

            // Index FastCDC sub-blocks to deduplicate shared engine chunks across patched versions.
            let cdc = fastcdc::v2020::FastCDC::new(
                &raw,
                FASTCDC_MIN_SIZE,
                FASTCDC_AVG_SIZE,
                FASTCDC_MAX_SIZE,
            );
            for ch in cdc {
                let slice = &raw[ch.offset..ch.offset + ch.length];
                let ch_hash = blake3::hash(slice).to_hex().to_string();
                if !unique_blocks.insert(ch_hash.clone()) {
                    deduplicated_bytes = deduplicated_bytes.saturating_add(ch.length as u64);
                } else {
                    let _ = store_cas_block(cas_root, slice);
                }
            }

            if let Some(first_path) = whole_file_owners.get(&file_hash) {
                if first_path != path {
                    let backup = path.with_extension("duke_dedup_tmp");
                    if fs::rename(path, &backup).is_ok() {
                        if duke_core::win32_ffi::create_hard_link(path, first_path).is_ok() {
                            let _ = fs::remove_file(&backup);
                            hardlinked_files += 1;
                        } else {
                            let _ = fs::rename(&backup, path);
                        }
                    }
                }
            } else {
                whole_file_owners.insert(file_hash, path.to_path_buf());
            }
        }
    }

    Ok(CasDedupReport {
        cas_root: cas_root.to_string_lossy().into_owned(),
        scanned_games,
        scanned_files,
        unique_blocks: unique_blocks.len(),
        total_logical_bytes,
        deduplicated_bytes,
        hardlinked_files,
        summary: format!(
            "Indexed {} engine files across {} game(s): {} unique FastCDC blocks, {} deduplicated ({} full files hardlinked).",
            scanned_files,
            scanned_games,
            unique_blocks.len(),
            crate::format_bytes(deduplicated_bytes),
            hardlinked_files
        ),
    })
}

// =========================================================================
// Windows 11 ProjectedFSLib.dll Dynamic Loader & Virtual Namespace Host
// =========================================================================

#[cfg(target_os = "windows")]
mod win_projfs {
    use super::*;

    pub type Hresult = i32;
    pub const S_OK: Hresult = 0;
    pub const ERROR_FILE_NOT_FOUND_HR: Hresult = 0x8007_0002u32 as i32;
    pub const ERROR_INSUFFICIENT_BUFFER_HR: Hresult = 0x8007_007Au32 as i32;
    pub const PRJ_CB_DATA_FLAG_ENUM_RESTART_SCAN: u32 = 0x0000_0001;
    pub const PRJ_CB_DATA_FLAG_ENUM_RETURN_SINGLE_ENTRY: u32 = 0x0000_0002;
    pub const PRJ_FLAG_USE_NEGATIVE_PATH_CACHE: u32 = 0x0000_0001;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct PrjPlaceholderVersionInfo {
        pub provider_id: [u8; 128],
        pub content_id: [u8; 128],
    }

    impl Default for PrjPlaceholderVersionInfo {
        fn default() -> Self {
            Self {
                provider_id: [0u8; 128],
                content_id: [0u8; 128],
            }
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct PrjFileBasicInfo {
        pub is_directory: u8,
        pub file_size: i64,
        pub creation_time: i64,
        pub last_access_time: i64,
        pub last_write_time: i64,
        pub change_time: i64,
        pub file_attributes: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct PrjEaInformation {
        pub ea_buffer_size: u32,
        pub offset_to_first_ea: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct PrjSecurityInformation {
        pub security_buffer_size: u32,
        pub offset_to_security_descriptor: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct PrjStreamsInformation {
        pub streams_info_buffer_size: u32,
        pub offset_to_first_stream_info: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct PrjPlaceholderInfo {
        pub file_basic_info: PrjFileBasicInfo,
        pub ea_information: PrjEaInformation,
        pub security_information: PrjSecurityInformation,
        pub streams_information: PrjStreamsInformation,
        pub version_info: PrjPlaceholderVersionInfo,
        pub variable_data: [u8; 1],
    }

    #[repr(C)]
    pub struct PrjCallbackData {
        pub size: u32,
        pub flags: u32,
        pub namespace_virtualization_context: *mut c_void,
        pub command_id: i32,
        pub file_id: GUID,
        pub data_stream_id: GUID,
        pub file_path_name: PCWSTR,
        pub version_info: *mut PrjPlaceholderVersionInfo,
        pub triggering_process_id: u32,
        pub triggering_process_image_file_name: PCWSTR,
        pub instance_context: *mut c_void,
    }

    pub type StartDirEnumCb = unsafe extern "system" fn(*const PrjCallbackData, *const GUID) -> Hresult;
    pub type EndDirEnumCb = unsafe extern "system" fn(*const PrjCallbackData, *const GUID) -> Hresult;
    pub type GetDirEnumCb =
        unsafe extern "system" fn(*const PrjCallbackData, *const GUID, PCWSTR, *mut c_void) -> Hresult;
    pub type GetPlaceholderCb = unsafe extern "system" fn(*const PrjCallbackData) -> Hresult;
    pub type GetFileDataCb = unsafe extern "system" fn(*const PrjCallbackData, u64, u32) -> Hresult;

    #[repr(C)]
    pub struct PrjCallbacks {
        pub start_directory_enumeration_callback: Option<StartDirEnumCb>,
        pub end_directory_enumeration_callback: Option<EndDirEnumCb>,
        pub get_directory_enumeration_callback: Option<GetDirEnumCb>,
        pub get_placeholder_info_callback: Option<GetPlaceholderCb>,
        pub get_file_data_callback: Option<GetFileDataCb>,
        pub query_file_name_callback: *const c_void,
        pub notification_callback: *const c_void,
        pub cancel_command_callback: *const c_void,
    }

    #[repr(C)]
    pub struct PrjStartVirtualizingOptions {
        pub flags: u32,
        pub pool_thread_count: u32,
        pub concurrent_thread_count: u32,
        pub notification_mappings: *const c_void,
        pub notification_mappings_count: u32,
    }

    pub type FnPrjStartVirtualizing = unsafe extern "system" fn(
        PCWSTR,
        *const PrjCallbacks,
        *const c_void,
        *const PrjStartVirtualizingOptions,
        *mut *mut c_void,
    ) -> Hresult;
    pub type FnPrjStopVirtualizing = unsafe extern "system" fn(*mut c_void);
    pub type FnPrjMarkDirectoryAsPlaceholder = unsafe extern "system" fn(
        PCWSTR,
        PCWSTR,
        *const PrjPlaceholderVersionInfo,
        *const GUID,
    ) -> Hresult;
    pub type FnPrjWritePlaceholderInfo =
        unsafe extern "system" fn(*mut c_void, PCWSTR, *const PrjPlaceholderInfo, u32) -> Hresult;
    pub type FnPrjWriteFileData =
        unsafe extern "system" fn(*mut c_void, *const GUID, *const c_void, u64, u32) -> Hresult;
    pub type FnPrjFillDirEntryBuffer =
        unsafe extern "system" fn(PCWSTR, *const PrjFileBasicInfo, *mut c_void) -> Hresult;
    pub type FnPrjAllocateAlignedBuffer = unsafe extern "system" fn(*mut c_void, usize) -> *mut c_void;
    pub type FnPrjFreeAlignedBuffer = unsafe extern "system" fn(*mut c_void);
    pub type FnPrjFileNameMatch = unsafe extern "system" fn(PCWSTR, PCWSTR) -> u8;

    pub struct ProjFsApiTable {
        _module: HMODULE,
        pub start_virtualizing: FnPrjStartVirtualizing,
        pub stop_virtualizing: FnPrjStopVirtualizing,
        pub mark_dir_placeholder: FnPrjMarkDirectoryAsPlaceholder,
        pub write_placeholder_info: FnPrjWritePlaceholderInfo,
        pub write_file_data: FnPrjWriteFileData,
        pub fill_dir_entry_buffer: FnPrjFillDirEntryBuffer,
        pub allocate_aligned_buffer: FnPrjAllocateAlignedBuffer,
        pub free_aligned_buffer: FnPrjFreeAlignedBuffer,
        pub file_name_match: FnPrjFileNameMatch,
    }

    unsafe impl Send for ProjFsApiTable {}
    unsafe impl Sync for ProjFsApiTable {}

    static PROJFS_API: OnceLock<Option<ProjFsApiTable>> = OnceLock::new();

    unsafe fn sym<T: Copy>(h: HMODULE, name: &[u8]) -> Option<T> {
        let p = GetProcAddress(h, windows::core::PCSTR::from_raw(name.as_ptr()))?;
        Some(std::mem::transmute_copy(&p))
    }

    pub fn load_projfs_api() -> Option<&'static ProjFsApiTable> {
        PROJFS_API
            .get_or_init(|| {
                let wide: Vec<u16> = std::ffi::OsStr::new("ProjectedFSLib.dll")
                    .encode_wide()
                    .chain(Some(0))
                    .collect();
                let h = unsafe { LoadLibraryW(PCWSTR::from_raw(wide.as_ptr())).ok()? };
                unsafe {
                    Some(ProjFsApiTable {
                        _module: h,
                        start_virtualizing: sym(h, b"PrjStartVirtualizing\0")?,
                        stop_virtualizing: sym(h, b"PrjStopVirtualizing\0")?,
                        mark_dir_placeholder: sym(h, b"PrjMarkDirectoryAsPlaceholder\0")?,
                        write_placeholder_info: sym(h, b"PrjWritePlaceholderInfo\0")?,
                        write_file_data: sym(h, b"PrjWriteFileData\0")?,
                        fill_dir_entry_buffer: sym(h, b"PrjFillDirEntryBuffer\0")?,
                        allocate_aligned_buffer: sym(h, b"PrjAllocateAlignedBuffer\0")?,
                        free_aligned_buffer: sym(h, b"PrjFreeAlignedBuffer\0")?,
                        file_name_match: sym(h, b"PrjFileNameMatch\0")?,
                    })
                }
            })
            .as_ref()
    }

    pub fn utf16_z(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(Some(0))
            .collect()
    }

    pub unsafe fn pcwstr_to_string(ptr: PCWSTR) -> String {
        if ptr.is_null() {
            return String::new();
        }
        let mut len = 0usize;
        while *ptr.0.add(len) != 0 && len < 4096 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(ptr.0, len))
    }

    #[derive(Clone)]
    struct EnumSession {
        entries: Vec<(String, bool, i64)>,
        cursor: usize,
        last_pattern: Option<String>,
    }

    pub struct VirtualNamespaceContext {
        pub mount_id: String,
        pub game_title: String,
        pub mount_dir: PathBuf,
        pub backing_url: String,
        pub part_path: PathBuf,
        pub cas_root: PathBuf,
        pub total_bytes: u64,
        pub coordinator: Option<Arc<ProjFsPriorityCoordinator>>,
        pub files: BTreeMap<String, VirtualFileSpan>,
        pub dirs: HashMap<String, Vec<(String, bool, i64)>>,
        enums: Mutex<HashMap<[u8; 16], EnumSession>>,
    }

    fn normalize_rel(raw: &str) -> String {
        raw.replace('/', "\\").trim_matches('\\').to_ascii_lowercase()
    }

    fn guid_key(g: &GUID) -> [u8; 16] {
        let mut out = [0u8; 16];
        out[0..4].copy_from_slice(&g.data1.to_le_bytes());
        out[4..6].copy_from_slice(&g.data2.to_le_bytes());
        out[6..8].copy_from_slice(&g.data3.to_le_bytes());
        out[8..16].copy_from_slice(&g.data4);
        out
    }

    type VirtualDirIndex = (BTreeMap<String, VirtualFileSpan>, HashMap<String, Vec<(String, bool, i64)>>);

    pub fn build_dir_index(files: &[VirtualFileSpan]) -> VirtualDirIndex {
        let mut file_map = BTreeMap::new();
        let mut dir_children: HashMap<String, HashMap<String, (bool, i64)>> = HashMap::new();
        dir_children.entry(String::new()).or_default();

        for f in files {
            let rel = f.rel_path.replace('/', "\\").trim_matches('\\').to_string();
            if rel.is_empty() {
                continue;
            }
            let key = rel.to_ascii_lowercase();
            let size_i64 = f.file_size as i64;
            file_map.insert(key, f.clone());

            let parts: Vec<&str> = rel.split('\\').filter(|p| !p.is_empty()).collect();
            let mut parent = String::new();
            for (idx, part) in parts.iter().enumerate() {
                let is_dir = idx + 1 < parts.len();
                let entry_size = if is_dir { 0 } else { size_i64 };
                dir_children
                    .entry(parent.clone())
                    .or_default()
                    .insert((*part).to_string(), (is_dir, entry_size));
                if is_dir {
                    parent = if parent.is_empty() {
                        part.to_ascii_lowercase()
                    } else {
                        format!("{}\\{}", parent, part.to_ascii_lowercase())
                    };
                    dir_children.entry(parent.clone()).or_default();
                }
            }
        }

        let mut dirs = HashMap::with_capacity(dir_children.len());
        for (dir_key, map) in dir_children {
            let mut list: Vec<(String, bool, i64)> = map
                .into_iter()
                .map(|(name, (is_dir, size))| (name, is_dir, size))
                .collect();
            list.sort_by_key(|a| a.0.to_ascii_lowercase());
            dirs.insert(dir_key, list);
        }
        (file_map, dirs)
    }

    unsafe extern "system" fn cb_start_dir_enum(
        data: *const PrjCallbackData,
        enum_id: *const GUID,
    ) -> Hresult {
        let Some(d) = data.as_ref() else {
            return -1;
        };
        let Some(eid) = enum_id.as_ref() else {
            return -1;
        };
        let ctx = &*(d.instance_context as *const VirtualNamespaceContext);
        let dir_key = normalize_rel(&pcwstr_to_string(d.file_path_name));
        let entries = ctx.dirs.get(&dir_key).cloned().unwrap_or_default();
        if let Ok(mut guard) = ctx.enums.lock() {
            guard.insert(
                guid_key(eid),
                EnumSession {
                    entries,
                    cursor: 0,
                    last_pattern: None,
                },
            );
        }
        S_OK
    }

    unsafe extern "system" fn cb_end_dir_enum(
        data: *const PrjCallbackData,
        enum_id: *const GUID,
    ) -> Hresult {
        if let (Some(d), Some(eid)) = (data.as_ref(), enum_id.as_ref()) {
            let ctx = &*(d.instance_context as *const VirtualNamespaceContext);
            if let Ok(mut guard) = ctx.enums.lock() {
                guard.remove(&guid_key(eid));
            }
        }
        S_OK
    }

    unsafe extern "system" fn cb_get_dir_enum(
        data: *const PrjCallbackData,
        enum_id: *const GUID,
        search_expr: PCWSTR,
        buf_handle: *mut c_void,
    ) -> Hresult {
        let (Some(d), Some(eid), Some(api)) = (data.as_ref(), enum_id.as_ref(), load_projfs_api()) else {
            return -1;
        };
        let ctx = &*(d.instance_context as *const VirtualNamespaceContext);
        let Ok(mut guard) = ctx.enums.lock() else {
            return -1;
        };
        let Some(sess) = guard.get_mut(&guid_key(eid)) else {
            return ERROR_FILE_NOT_FOUND_HR;
        };
        let pattern = if search_expr.is_null() {
            None
        } else {
            let s = pcwstr_to_string(search_expr);
            if s.is_empty() { None } else { Some(s) }
        };
        if (d.flags & PRJ_CB_DATA_FLAG_ENUM_RESTART_SCAN) != 0 {
            sess.cursor = 0;
            sess.last_pattern = pattern.clone();
        }
        let active_pat = pattern.or_else(|| sess.last_pattern.clone());
        let pat_w = active_pat.as_deref().map(utf16_z);

        while sess.cursor < sess.entries.len() {
            let (ref name, is_dir, size) = sess.entries[sess.cursor];
            let name_w = utf16_z(name);
            if let Some(ref pw) = pat_w {
                if (api.file_name_match)(PCWSTR::from_raw(name_w.as_ptr()), PCWSTR::from_raw(pw.as_ptr())) == 0 {
                    sess.cursor += 1;
                    continue;
                }
            }
            let basic = PrjFileBasicInfo {
                is_directory: u8::from(is_dir),
                file_size: size,
                file_attributes: if is_dir { 0x10 } else { 0x80 },
                ..Default::default()
            };
            let hr = (api.fill_dir_entry_buffer)(PCWSTR::from_raw(name_w.as_ptr()), &basic, buf_handle);
            if hr == ERROR_INSUFFICIENT_BUFFER_HR {
                break;
            }
            sess.cursor += 1;
            if (d.flags & PRJ_CB_DATA_FLAG_ENUM_RETURN_SINGLE_ENTRY) != 0 {
                break;
            }
        }
        S_OK
    }

    unsafe extern "system" fn cb_get_placeholder_info(data: *const PrjCallbackData) -> Hresult {
        let (Some(d), Some(api)) = (data.as_ref(), load_projfs_api()) else {
            return -1;
        };
        let ctx = &*(d.instance_context as *const VirtualNamespaceContext);
        let rel = normalize_rel(&pcwstr_to_string(d.file_path_name));

        let (is_dir, file_size) = if ctx.dirs.contains_key(&rel) {
            (true, 0i64)
        } else if let Some(span) = ctx.files.get(&rel) {
            (false, span.file_size as i64)
        } else {
            return ERROR_FILE_NOT_FOUND_HR;
        };

        let ph = PrjPlaceholderInfo {
            file_basic_info: PrjFileBasicInfo {
                is_directory: u8::from(is_dir),
                file_size,
                file_attributes: if is_dir { 0x10 } else { 0x80 },
                ..Default::default()
            },
            ..Default::default()
        };
        let dest_w = utf16_z(&pcwstr_to_string(d.file_path_name));
        (api.write_placeholder_info)(
            d.namespace_virtualization_context,
            PCWSTR::from_raw(dest_w.as_ptr()),
            &ph,
            std::mem::size_of::<PrjPlaceholderInfo>() as u32,
        )
    }

    unsafe extern "system" fn cb_get_file_data(
        data: *const PrjCallbackData,
        byte_offset: u64,
        length: u32,
    ) -> Hresult {
        let (Some(d), Some(api)) = (data.as_ref(), load_projfs_api()) else {
            return -1;
        };
        let ctx = &*(d.instance_context as *const VirtualNamespaceContext);
        let rel = normalize_rel(&pcwstr_to_string(d.file_path_name));
        let Some(span) = ctx.files.get(&rel) else {
            return ERROR_FILE_NOT_FOUND_HR;
        };
        if let Some(ref coord) = ctx.coordinator {
            coord.record_on_demand_read();
        }

        let mut payload = vec![0u8; length as usize];
        let mut filled = 0usize;

        // First resolve FastCDC CAS blocks already materialized across any installed game.
        if !span.chunks.is_empty() {
            for ch in &span.chunks {
                let ch_start = ch.file_offset;
                let ch_end = ch_start.saturating_add(ch.length as u64);
                let req_end = byte_offset.saturating_add(length as u64);
                if ch_end <= byte_offset || ch_start >= req_end {
                    continue;
                }
                if let Some(block) = read_cas_block(&ctx.cas_root, &ch.blake3) {
                    if let Some(ref coord) = ctx.coordinator {
                        coord.record_cas_hit();
                    }
                    let copy_start = byte_offset.max(ch_start);
                    let copy_end = req_end.min(ch_end);
                    let src_off = (copy_start - ch_start) as usize;
                    let dst_off = (copy_start - byte_offset) as usize;
                    let cnt = (copy_end - copy_start) as usize;
                    if src_off + cnt <= block.len() && dst_off + cnt <= payload.len() {
                        payload[dst_off..dst_off + cnt].copy_from_slice(&block[src_off..src_off + cnt]);
                        filled += cnt;
                    }
                }
            }
        }

        // For any missing bytes, enqueue high-priority AIMD ranges and wait up to 8s before reading `.part`.
        if filled < payload.len() {
            let ranges = map_virtual_read_to_stream_ranges(span, byte_offset, length);
            if let Some(ref coord) = ctx.coordinator {
                for &(rs, re, _) in &ranges {
                    coord.enqueue_urgent_range(rs, re);
                }
                for &(rs, re, _) in &ranges {
                    let _ = coord.wait_for_range(rs, re, std::time::Duration::from_secs(8));
                }
            }
            if ctx.part_path.is_file() {
                if let Ok(mut pf) = File::open(&ctx.part_path) {
                    let stream_pos = span.stream_offset.saturating_add(byte_offset);
                    if pf.seek(SeekFrom::Start(stream_pos)).is_ok() {
                        let _ = pf.read_exact(&mut payload);
                    }
                }
            }
        }

        let aligned = (api.allocate_aligned_buffer)(d.namespace_virtualization_context, payload.len());
        if aligned.is_null() {
            return -1;
        }
        std::ptr::copy_nonoverlapping(payload.as_ptr(), aligned as *mut u8, payload.len());
        let hr = (api.write_file_data)(
            d.namespace_virtualization_context,
            &d.data_stream_id,
            aligned,
            byte_offset,
            length,
        );
        (api.free_aligned_buffer)(aligned);
        hr
    }

    pub struct ActiveProjFsMount {
        pub handle: *mut c_void,
        pub ctx: *mut VirtualNamespaceContext,
    }

    unsafe impl Send for ActiveProjFsMount {}
    unsafe impl Sync for ActiveProjFsMount {}

    static ACTIVE_MOUNTS: OnceLock<Mutex<HashMap<String, ActiveProjFsMount>>> = OnceLock::new();

    pub fn active_mounts() -> &'static Mutex<HashMap<String, ActiveProjFsMount>> {
        ACTIVE_MOUNTS.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub struct ProjFsMountSpec {
        pub mount_id: String,
        pub game_title: String,
        pub mount_dir: PathBuf,
        pub backing_url: String,
        pub part_path: PathBuf,
        pub total_bytes: u64,
        pub coordinator: Option<Arc<ProjFsPriorityCoordinator>>,
        pub files: Vec<VirtualFileSpan>,
    }

    pub fn start_mount(spec: ProjFsMountSpec) -> Result<ProjFsMountInfo, String> {
        let ProjFsMountSpec {
            mount_id,
            game_title,
            mount_dir,
            backing_url,
            part_path,
            total_bytes,
            coordinator,
            files,
        } = spec;
        let api = load_projfs_api().ok_or_else(|| {
            "ProjectedFSLib.dll is not enabled on this Windows installation. Enable Client-ProjFS in Advanced Settings.".to_string()
        })?;
        fs::create_dir_all(&mount_dir)
            .map_err(|e| format!("Failed creating ProjFS mount root {}: {e}", mount_dir.display()))?;

        let instance_guid = GUID::from_u128(
            u128::from_le_bytes(blake3::hash(mount_id.as_bytes()).as_bytes()[..16].try_into().unwrap()),
        );
        let root_w = utf16_z(&mount_dir.to_string_lossy());
        let ver = PrjPlaceholderVersionInfo::default();
        let hr_mark = unsafe {
            (api.mark_dir_placeholder)(
                PCWSTR::from_raw(root_w.as_ptr()),
                PCWSTR::null(),
                &ver,
                &instance_guid,
            )
        };
        if hr_mark < 0 {
            return Err(format!("PrjMarkDirectoryAsPlaceholder failed (HRESULT 0x{hr_mark:08X})"));
        }

        let (file_map, dirs) = build_dir_index(&files);
        let ctx_box = Box::new(VirtualNamespaceContext {
            mount_id: mount_id.clone(),
            game_title: game_title.clone(),
            mount_dir: mount_dir.clone(),
            backing_url: backing_url.clone(),
            part_path,
            cas_root: shared_cas_root_dir(),
            total_bytes,
            coordinator: coordinator.clone(),
            files: file_map,
            dirs,
            enums: Mutex::new(HashMap::new()),
        });
        let ctx_ptr = Box::into_raw(ctx_box);

        let callbacks = PrjCallbacks {
            start_directory_enumeration_callback: Some(cb_start_dir_enum),
            end_directory_enumeration_callback: Some(cb_end_dir_enum),
            get_directory_enumeration_callback: Some(cb_get_dir_enum),
            get_placeholder_info_callback: Some(cb_get_placeholder_info),
            get_file_data_callback: Some(cb_get_file_data),
            query_file_name_callback: std::ptr::null(),
            notification_callback: std::ptr::null(),
            cancel_command_callback: std::ptr::null(),
        };
        let opts = PrjStartVirtualizingOptions {
            flags: PRJ_FLAG_USE_NEGATIVE_PATH_CACHE,
            pool_thread_count: 8,
            concurrent_thread_count: 4,
            notification_mappings: std::ptr::null(),
            notification_mappings_count: 0,
        };
        let mut ns_ctx: *mut c_void = std::ptr::null_mut();
        let hr = unsafe {
            (api.start_virtualizing)(
                PCWSTR::from_raw(root_w.as_ptr()),
                &callbacks,
                ctx_ptr as *const c_void,
                &opts,
                &mut ns_ctx,
            )
        };
        if hr < 0 || ns_ctx.is_null() {
            unsafe { drop(Box::from_raw(ctx_ptr)) };
            return Err(format!("PrjStartVirtualizing failed (HRESULT 0x{hr:08X})"));
        }

        if let Ok(mut guard) = active_mounts().lock() {
            if let Some(prev) = guard.remove(&mount_id) {
                unsafe {
                    (api.stop_virtualizing)(prev.handle);
                    drop(Box::from_raw(prev.ctx));
                }
            }
            guard.insert(
                mount_id.clone(),
                ActiveProjFsMount {
                    handle: ns_ctx,
                    ctx: ctx_ptr,
                },
            );
        }

        Ok(ProjFsMountInfo {
            mount_id,
            game_title,
            mount_dir: mount_dir.to_string_lossy().into_owned(),
            backing_url,
            total_bytes,
            downloaded_bytes: 0,
            ready_to_play_early: false,
            on_demand_reads: 0,
            cas_cache_hits: 0,
            remote_prioritized_bytes: 0,
        })
    }

    pub fn stop_mount(mount_id: &str) -> Result<bool, String> {
        let Some(api) = load_projfs_api() else {
            return Ok(false);
        };
        let removed = active_mounts()
            .lock()
            .map_err(|_| "ProjFS mount registry lock poisoned".to_string())?
            .remove(mount_id);
        if let Some(m) = removed {
            unsafe {
                (api.stop_virtualizing)(m.handle);
                drop(Box::from_raw(m.ctx));
            }
            return Ok(true);
        }
        Ok(false)
    }

    pub fn list_mounts() -> Vec<ProjFsMountInfo> {
        let Ok(guard) = active_mounts().lock() else {
            return Vec::new();
        };
        guard
            .values()
            .map(|m| unsafe {
                let c = &*m.ctx;
                let (reads, hits, prio, ready) = if let Some(ref coord) = c.coordinator {
                    (
                        coord.on_demand_reads.load(Ordering::Relaxed),
                        coord.cas_cache_hits.load(Ordering::Relaxed),
                        coord.remote_prioritized_bytes.load(Ordering::Relaxed),
                        coord.core_threshold_reached.load(Ordering::Relaxed),
                    )
                } else {
                    (0, 0, 0, false)
                };
                ProjFsMountInfo {
                    mount_id: c.mount_id.clone(),
                    game_title: c.game_title.clone(),
                    mount_dir: c.mount_dir.to_string_lossy().into_owned(),
                    backing_url: c.backing_url.clone(),
                    total_bytes: c.total_bytes,
                    downloaded_bytes: 0,
                    ready_to_play_early: ready,
                    on_demand_reads: reads,
                    cas_cache_hits: hits,
                    remote_prioritized_bytes: prio,
                }
            })
            .collect()
    }
}

pub fn probe_projfs_status() -> ProjFsRuntimeStatus {
    let cas_root = shared_cas_root_dir().to_string_lossy().into_owned();
    #[cfg(target_os = "windows")]
    {
        let sys32 = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        let dll_path = PathBuf::from(sys32).join("System32").join("ProjectedFSLib.dll");
        let dll_present = dll_path.is_file();
        let api_bound = win_projfs::load_projfs_api().is_some();
        let active_mounts = win_projfs::list_mounts();
        let message = if api_bound {
            format!(
                "ProjectedFSLib.dll bound ({} active virtual mount{}).",
                active_mounts.len(),
                if active_mounts.len() == 1 { "" } else { "s" }
            )
        } else {
            "Windows Projected File System optional feature (Client-ProjFS) is not active yet."
                .to_string()
        };
        ProjFsRuntimeStatus {
            dll_present,
            api_bound,
            feature_enabled_hint: dll_present && api_bound,
            cas_root_dir: cas_root,
            active_mounts,
            message,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        ProjFsRuntimeStatus {
            dll_present: false,
            api_bound: false,
            feature_enabled_hint: false,
            cas_root_dir: cas_root,
            active_mounts: Vec::new(),
            message: "ProjectedFS is only supported on Windows 10/11.".to_string(),
        }
    }
}

#[tauri::command]
pub async fn get_projfs_runtime_status() -> Result<ProjFsRuntimeStatus, String> {
    Ok(probe_projfs_status())
}

#[tauri::command]
pub async fn enable_projfs_windows_feature() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        let status_before = probe_projfs_status();
        if status_before.api_bound {
            return Ok("Windows Projected File System (Client-ProjFS) is already enabled and bound."
                .to_string());
        }
        tokio::task::spawn_blocking(|| {
            let mut cmd = std::process::Command::new("powershell");
            cmd.creation_flags(crate::CREATE_NO_WINDOW);
            let output = cmd
                .args([
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-Command",
                    "Start-Process powershell -Verb RunAs -Wait -ArgumentList '-NoProfile -ExecutionPolicy Bypass -Command \"Enable-WindowsOptionalFeature -Online -FeatureName Client-ProjFS -NoRestart\"'",
                ])
                .output()
                .map_err(|e| format!("Failed launching PowerShell feature installer: {e}"))?;
            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                return Err(format!("ProjFS activation cancelled or failed: {}", err.trim()));
            }
            Ok(
                "Requested Windows Client-ProjFS activation. Restart Duke Launcher (or Windows if prompted) if ProjectedFSLib.dll does not load immediately."
                    .to_string(),
            )
        })
        .await
        .map_err(|e| format!("ProjFS activation task failed: {e}"))?
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Projected File System is only supported on Windows 11.".to_string())
    }
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn mount_projfs_virtual_game(
    app: tauri::AppHandle,
    game_id: String,
    game_title: String,
    mount_dir: String,
    backing_url: Option<String>,
    backing_file_path: Option<String>,
    total_bytes: Option<u64>,
    job_id: Option<String>,
    files: Vec<VirtualFileSpan>,
) -> Result<ProjFsMountInfo, String> {
    #[cfg(target_os = "windows")]
    {
        let coord = job_id
            .as_deref()
            .and_then(get_job_coordinator)
            .or_else(|| total_bytes.map(|sz| register_job_coordinator(&game_id, sz)));
        let part_path = backing_file_path
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(&mount_dir).with_extension("part"));
        let computed_total = total_bytes
            .filter(|&v| v > 0)
            .unwrap_or_else(|| files.iter().map(|f| f.file_size).sum());
        let info = win_projfs::start_mount(win_projfs::ProjFsMountSpec {
            mount_id: game_id,
            game_title,
            mount_dir: PathBuf::from(mount_dir),
            backing_url: backing_url.unwrap_or_default(),
            part_path,
            total_bytes: computed_total,
            coordinator: coord,
            files,
        })?;
        let _ = app.emit("projfs-mount-changed", &info);
        Ok(info)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, game_id, game_title, mount_dir, backing_url, backing_file_path, total_bytes, job_id, files);
        Err("ProjFS mount is only available on Windows.".to_string())
    }
}

#[tauri::command]
pub async fn unmount_projfs_virtual_game(
    app: tauri::AppHandle,
    mount_id: String,
) -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        let stopped = win_projfs::stop_mount(&mount_id)?;
        let _ = app.emit("projfs-mount-unmounted", serde_json::json!({ "mountId": mount_id, "stopped": stopped }));
        Ok(stopped)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, mount_id);
        Ok(false)
    }
}

#[tauri::command]
pub async fn deduplicate_library_cas(app: tauri::AppHandle) -> Result<CasDedupReport, String> {
    let lib = crate::load_library_lightweight(&app)?;
    let dirs: Vec<PathBuf> = lib
        .into_iter()
        .map(|g| PathBuf::from(&g.install_dir))
        .filter(|p| p.is_dir())
        .collect();
    let cas_root = shared_cas_root_dir();
    tokio::task::spawn_blocking(move || deduplicate_game_dirs_cas(&dirs, &cas_root))
        .await
        .map_err(|e| format!("CAS deduplication task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fastcdc_virtual_range_mapping_and_cas_dedup_work() {
        let root = std::env::temp_dir().join(format!("duke_projfs_cas_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let game_a = root.join("GameA");
        let game_b = root.join("GameB");
        let cas = root.join(".duke-cas");
        fs::create_dir_all(&game_a).unwrap();
        fs::create_dir_all(&game_b).unwrap();

        let shared_blob = vec![0x5Au8; 128 * 1024];
        let pak_a = game_a.join("pakchunk0-Windows.pak");
        let pak_b = game_b.join("pakchunk0-Windows.pak");
        fs::write(&pak_a, &shared_blob).unwrap();
        fs::write(&pak_b, &shared_blob).unwrap();

        let (sz, chunks) = index_file_into_cas(&cas, &pak_a, 4096).unwrap();
        assert_eq!(sz, shared_blob.len() as u64);
        assert!(!chunks.is_empty());

        let span = VirtualFileSpan {
            rel_path: "Content\\Paks\\pakchunk0-Windows.pak".to_string(),
            file_size: sz,
            stream_offset: 4096,
            chunks,
        };
        let ranges = map_virtual_read_to_stream_ranges(&span, 1024, 32 * 1024);
        assert!(!ranges.is_empty());
        assert!(ranges[0].0 >= 4096);
        assert!(ranges.iter().all(|(_, _, h)| !h.is_empty()));

        let report = deduplicate_game_dirs_cas(&[game_a.clone(), game_b.clone()], &cas).unwrap();
        assert_eq!(report.scanned_games, 2);
        assert_eq!(report.scanned_files, 2);
        assert_eq!(report.hardlinked_files, 1);
        assert!(report.deduplicated_bytes >= shared_blob.len() as u64);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn priority_range_queue_coalesces_and_signals_completion() {
        let coord = ProjFsPriorityCoordinator::new("job-1", 1_000_000);
        coord.enqueue_urgent_range(2000, 4000);
        coord.enqueue_urgent_range(3500, 6000);
        coord.enqueue_urgent_range(100, 500);

        let r1 = coord.pop_urgent_range().unwrap();
        assert_eq!((r1.start, r1.end), (100, 500));
        let r2 = coord.pop_urgent_range().unwrap();
        assert_eq!((r2.start, r2.end), (2000, 6000));

        assert!(!coord.is_range_completed(0, 1024));
        coord.mark_range_completed(0, 2048, true);
        assert!(coord.is_range_completed(0, 1024));
        assert!(coord.wait_for_range(512, 1500, std::time::Duration::from_millis(50)));
        assert!(coord.update_progress(150_000));
        assert!(!coord.update_progress(200_000));
    }
}
