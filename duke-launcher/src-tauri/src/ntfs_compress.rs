use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;
use tauri::{AppHandle, Emitter};
use walkdir::WalkDir;

pub const FSCTL_SET_EXTERNAL_BACKING: u32 = 0x0009_030C;
pub const FSCTL_DELETE_EXTERNAL_BACKING: u32 = 0x0009_0310;
pub const FSCTL_GET_EXTERNAL_BACKING: u32 = 0x0009_0314;

pub const WOF_CURRENT_VERSION: u32 = 1;
pub const WOF_PROVIDER_FILE: u32 = 2;
pub const FILE_PROVIDER_CURRENT_VERSION: u32 = 1;

pub const WOF_ALG_XPRESS4K: u32 = 0;
pub const WOF_ALG_LZX: u32 = 1;
pub const WOF_ALG_XPRESS8K: u32 = 2;
pub const WOF_ALG_XPRESS16K: u32 = 3;

pub const WOF_MIN_FILE_SIZE_BYTES: u64 = 64 * 1024 + 1; // strictly > 64 KiB

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum WofAlgorithm {
    Xpress4k,
    Xpress8k,
    Xpress16k,
    Lzx,
}

impl WofAlgorithm {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw
            .unwrap_or("LZX")
            .trim()
            .to_ascii_uppercase()
            .as_str()
        {
            "XPRESS4K" => Self::Xpress4k,
            "XPRESS8K" => Self::Xpress8k,
            "XPRESS16K" => Self::Xpress16k,
            _ => Self::Lzx,
        }
    }

    pub const fn as_win32_id(self) -> u32 {
        match self {
            Self::Xpress4k => WOF_ALG_XPRESS4K,
            Self::Lzx => WOF_ALG_LZX,
            Self::Xpress8k => WOF_ALG_XPRESS8K,
            Self::Xpress16k => WOF_ALG_XPRESS16K,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Xpress4k => "XPRESS4K",
            Self::Xpress8k => "XPRESS8K",
            Self::Xpress16k => "XPRESS16K",
            Self::Lzx => "LZX",
        }
    }

    pub fn from_win32_id(id: u32) -> &'static str {
        match id {
            WOF_ALG_XPRESS4K => "XPRESS4K",
            WOF_ALG_LZX => "LZX",
            WOF_ALG_XPRESS8K => "XPRESS8K",
            WOF_ALG_XPRESS16K => "XPRESS16K",
            _ => "None",
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct WofExternalInfo {
    pub version: u32,
    pub provider: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct FileProviderExternalInfoV1 {
    pub version: u32,
    pub algorithm: u32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct WofFileCompressionPayload {
    pub external_info: WofExternalInfo,
    pub file_provider_info: FileProviderExternalInfoV1,
}

const _: () = assert!(std::mem::size_of::<WofExternalInfo>() == 8);
const _: () = assert!(std::mem::size_of::<FileProviderExternalInfoV1>() == 12);
const _: () = assert!(std::mem::size_of::<WofFileCompressionPayload>() == 20);

impl WofFileCompressionPayload {
    pub const fn new(algorithm: WofAlgorithm) -> Self {
        Self {
            external_info: WofExternalInfo {
                version: WOF_CURRENT_VERSION,
                provider: WOF_PROVIDER_FILE,
            },
            file_provider_info: FileProviderExternalInfoV1 {
                version: FILE_PROVIDER_CURRENT_VERSION,
                algorithm: algorithm.as_win32_id(),
                flags: 0,
            },
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn to_le_bytes(self) -> [u8; 20] {
        let mut out = [0u8; 20];
        out[0..4].copy_from_slice(&self.external_info.version.to_le_bytes());
        out[4..8].copy_from_slice(&self.external_info.provider.to_le_bytes());
        out[8..12].copy_from_slice(&self.file_provider_info.version.to_le_bytes());
        out[12..16].copy_from_slice(&self.file_provider_info.algorithm.to_le_bytes());
        out[16..20].copy_from_slice(&self.file_provider_info.flags.to_le_bytes());
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct GameStorageStats {
    pub game_id: String,
    pub game_dir: String,
    pub logical_bytes: u64,
    pub compressed_bytes: u64,
    pub saved_bytes: u64,
    pub saved_pct: f64,
    pub active_algorithm: String,
    pub compressed_files: usize,
    pub eligible_files: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct WofCompressProgress {
    pub game_id: String,
    pub algorithm: String,
    pub files_processed: usize,
    pub files_total: usize,
    pub logical_bytes: u64,
    pub compressed_bytes: u64,
    pub saved_bytes: u64,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct WofCompressionReport {
    pub game_id: String,
    pub algorithm: String,
    pub logical_bytes: u64,
    pub compressed_bytes: u64,
    pub saved_bytes: u64,
    pub saved_pct: f64,
    pub files_compressed: usize,
    pub files_skipped: usize,
    pub elapsed_ms: u64,
    pub message: String,
    pub library: Vec<crate::GameEntry>,
}

fn should_skip_walk_entry(entry: &walkdir::DirEntry, root: &Path) -> bool {
    if entry.path() == root {
        return false;
    }
    matches!(
        entry.file_name().to_str(),
        Some(".duke-cas" | ".duke-profile" | ".duke-patch-tmp")
    )
}

pub fn should_compress_file_size(logical_size: u64, on_disk_size: u64) -> bool {
    logical_size >= WOF_MIN_FILE_SIZE_BYTES && on_disk_size >= logical_size
}

#[cfg(target_os = "windows")]
mod win32_wof {
    use super::*;
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_GENERIC_READ, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES, GetCompressedFileSizeW,
        OPEN_EXISTING,
    };
    use windows::Win32::System::IO::DeviceIoControl;
    use windows::core::PCWSTR;

    struct HandleGuard(HANDLE);
    impl Drop for HandleGuard {
        fn drop(&mut self) {
            if self.0 != INVALID_HANDLE_VALUE && !self.0.is_invalid() {
                unsafe {
                    let _ = CloseHandle(self.0);
                }
            }
        }
    }

    fn open_for_wof(path: &Path) -> Result<HandleGuard, u32> {
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let share = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
        let handle = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                FILE_GENERIC_READ.0 | FILE_WRITE_ATTRIBUTES.0,
                share,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
        };
        match handle {
            Ok(h) => Ok(HandleGuard(h)),
            Err(_) => {
                let fallback = unsafe {
                    CreateFileW(
                        PCWSTR(wide.as_ptr()),
                        FILE_GENERIC_READ.0,
                        share,
                        None,
                        OPEN_EXISTING,
                        FILE_FLAG_BACKUP_SEMANTICS,
                        None,
                    )
                };
                fallback
                    .map(HandleGuard)
                    .map_err(|e| e.code().0 as u32)
            }
        }
    }

    pub fn physical_size_bytes(path: &Path) -> u64 {
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut high: u32 = 0;
        let low = unsafe { GetCompressedFileSizeW(PCWSTR(wide.as_ptr()), Some(&mut high)) };
        if low != u32::MAX {
            ((high as u64) << 32) | (low as u64)
        } else {
            fs::metadata(path).map(|m| m.len()).unwrap_or(0)
        }
    }

    pub fn query_wof_algorithm(path: &Path) -> Option<u32> {
        let guard = open_for_wof(path).ok()?;
        let mut out = WofFileCompressionPayload::default();
        let mut bytes_returned: u32 = 0;
        let ok = unsafe {
            DeviceIoControl(
                guard.0,
                FSCTL_GET_EXTERNAL_BACKING,
                None,
                0,
                Some((&mut out as *mut WofFileCompressionPayload).cast::<c_void>()),
                std::mem::size_of::<WofFileCompressionPayload>() as u32,
                Some(&mut bytes_returned),
                None,
            )
        };
        if ok.is_ok() && out.external_info.provider == WOF_PROVIDER_FILE {
            Some(out.file_provider_info.algorithm)
        } else {
            None
        }
    }

    pub fn set_wof_compression(path: &Path, alg: WofAlgorithm) -> Result<bool, String> {
        if let Some(existing_alg) = query_wof_algorithm(path) {
            if existing_alg == alg.as_win32_id() {
                return Ok(false);
            }
        }
        let guard = open_for_wof(path).map_err(|code| format!("CreateFileW failed (0x{code:08X})"))?;
        let payload = WofFileCompressionPayload::new(alg);
        let mut bytes_returned: u32 = 0;
        let res = unsafe {
            DeviceIoControl(
                guard.0,
                FSCTL_SET_EXTERNAL_BACKING,
                Some((&payload as *const WofFileCompressionPayload).cast::<c_void>()),
                std::mem::size_of::<WofFileCompressionPayload>() as u32,
                None,
                0,
                Some(&mut bytes_returned),
                None,
            )
        };
        match res {
            Ok(()) => Ok(true),
            Err(err) => {
                // ERROR_COMPRESSION_NOT_BENEFICIAL (344 / 0x80070158) or ERROR_FILE_SYSTEM_LIMITATION
                let code = err.code().0 as u32;
                let low = code & 0xFFFF;
                if low == 344 || low == 17 || low == 32 {
                    Ok(false)
                } else {
                    Err(format!("DeviceIoControl(FSCTL_SET_EXTERNAL_BACKING) 0x{code:08X}"))
                }
            }
        }
    }

    pub fn delete_wof_compression(path: &Path) -> Result<bool, String> {
        if query_wof_algorithm(path).is_none() {
            return Ok(false);
        }
        let guard = open_for_wof(path).map_err(|code| format!("CreateFileW failed (0x{code:08X})"))?;
        let mut bytes_returned: u32 = 0;
        let res = unsafe {
            DeviceIoControl(
                guard.0,
                FSCTL_DELETE_EXTERNAL_BACKING,
                None,
                0,
                None,
                0,
                Some(&mut bytes_returned),
                None,
            )
        };
        match res {
            Ok(()) => Ok(true),
            Err(err) => {
                let code = err.code().0 as u32;
                let low = code & 0xFFFF;
                if low == 342 || low == 1 {
                    Ok(false)
                } else {
                    Err(format!("DeviceIoControl(FSCTL_DELETE_EXTERNAL_BACKING) 0x{code:08X}"))
                }
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod win32_wof {
    use super::*;
    pub fn physical_size_bytes(path: &Path) -> u64 {
        fs::metadata(path).map(|m| m.len()).unwrap_or(0)
    }
    pub fn query_wof_algorithm(_path: &Path) -> Option<u32> {
        None
    }
    pub fn set_wof_compression(_path: &Path, _alg: WofAlgorithm) -> Result<bool, String> {
        Ok(false)
    }
    pub fn delete_wof_compression(_path: &Path) -> Result<bool, String> {
        Ok(false)
    }
}

pub fn measure_game_storage_stats(game_id: &str, game_dir: &Path) -> GameStorageStats {
    let mut logical_bytes = 0u64;
    let mut compressed_bytes = 0u64;
    let mut compressed_files = 0usize;
    let mut eligible_files = 0usize;
    let mut alg_counts: [usize; 4] = [0; 4];
    let mut sampled_wof = 0usize;

    for entry in WalkDir::new(game_dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !should_skip_walk_entry(e, game_dir))
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let logical = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let physical = win32_wof::physical_size_bytes(path);
        logical_bytes = logical_bytes.saturating_add(logical);
        compressed_bytes = compressed_bytes.saturating_add(physical);
        if logical >= WOF_MIN_FILE_SIZE_BYTES {
            eligible_files += 1;
            if physical < logical {
                compressed_files += 1;
                if sampled_wof < 16 {
                    sampled_wof += 1;
                    if let Some(alg_id) = win32_wof::query_wof_algorithm(path) {
                        if (alg_id as usize) < alg_counts.len() {
                            alg_counts[alg_id as usize] += 1;
                        }
                    }
                }
            }
        }
    }

    let saved_bytes = logical_bytes.saturating_sub(compressed_bytes);
    let saved_pct = if logical_bytes > 0 {
        ((saved_bytes as f64 / logical_bytes as f64) * 1000.0).round() / 10.0
    } else {
        0.0
    };
    let active_algorithm = alg_counts
        .iter()
        .enumerate()
        .filter(|(_, count)| **count > 0)
        .max_by_key(|(_, count)| *count)
        .map(|(id, _)| WofAlgorithm::from_win32_id(id as u32).to_string())
        .unwrap_or_else(|| {
            if compressed_files > 0 {
                "WOF".to_string()
            } else {
                "Uncompressed".to_string()
            }
        });

    GameStorageStats {
        game_id: game_id.to_string(),
        game_dir: game_dir.to_string_lossy().into_owned(),
        logical_bytes,
        compressed_bytes,
        saved_bytes,
        saved_pct,
        active_algorithm,
        compressed_files,
        eligible_files,
    }
}

pub fn run_wof_operation_sync(
    app: Option<&AppHandle>,
    game_id: &str,
    game_dir: &Path,
    compress: bool,
    algorithm: WofAlgorithm,
) -> WofCompressionReport {
    let started = Instant::now();
    let mut all_files: Vec<(PathBuf, u64)> = Vec::new();
    let mut pre_logical_total = 0u64;

    for entry in WalkDir::new(game_dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !should_skip_walk_entry(e, game_dir))
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let logical = entry.metadata().map(|m| m.len()).unwrap_or(0);
        pre_logical_total = pre_logical_total.saturating_add(logical);
        if logical >= WOF_MIN_FILE_SIZE_BYTES {
            all_files.push((entry.path().to_path_buf(), logical));
        }
    }

    let files_total = all_files.len();
    let queue = Mutex::new(all_files);
    let files_processed = AtomicU64::new(0);
    let files_changed = AtomicU64::new(0);
    let files_skipped = AtomicU64::new(0);
    let worker_count = std::thread::available_parallelism()
        .map(|n| n.get().clamp(2, 4))
        .unwrap_or(4);
    let stop_flag = AtomicBool::new(false);

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| {
                while !stop_flag.load(Ordering::Relaxed) {
                    let next = {
                        let mut guard = queue.lock().unwrap_or_else(|e| e.into_inner());
                        guard.pop()
                    };
                    let Some((path, logical)) = next else {
                        break;
                    };

                    let changed = if compress {
                        let physical = win32_wof::physical_size_bytes(&path);
                        if !should_compress_file_size(logical, physical)
                            && win32_wof::query_wof_algorithm(&path) == Some(algorithm.as_win32_id())
                        {
                            files_skipped.fetch_add(1, Ordering::Relaxed);
                            false
                        } else {
                            match win32_wof::set_wof_compression(&path, algorithm) {
                                Ok(true) => {
                                    files_changed.fetch_add(1, Ordering::Relaxed);
                                    true
                                }
                                Ok(false) | Err(_) => {
                                    files_skipped.fetch_add(1, Ordering::Relaxed);
                                    false
                                }
                            }
                        }
                    } else {
                        match win32_wof::delete_wof_compression(&path) {
                            Ok(true) => {
                                files_changed.fetch_add(1, Ordering::Relaxed);
                                true
                            }
                            Ok(false) | Err(_) => {
                                files_skipped.fetch_add(1, Ordering::Relaxed);
                                false
                            }
                        }
                    };

                    let _ = changed;
                    let done_now = files_processed.fetch_add(1, Ordering::Relaxed) + 1;
                    if let Some(app_h) = app {
                        if done_now == 1 || done_now.is_multiple_of(24) || done_now as usize == files_total {
                            let _ = app_h.emit(
                                "wof-compress-progress",
                                WofCompressProgress {
                                    game_id: game_id.to_string(),
                                    algorithm: if compress {
                                        algorithm.label().to_string()
                                    } else {
                                        "Decompress".to_string()
                                    },
                                    files_processed: done_now as usize,
                                    files_total,
                                    logical_bytes: pre_logical_total,
                                    compressed_bytes: 0,
                                    saved_bytes: 0,
                                    done: false,
                                },
                            );
                        }
                    }
                }
            });
        }
    });

    let stats = measure_game_storage_stats(game_id, game_dir);
    if let Some(app_h) = app {
        let _ = app_h.emit(
            "wof-compress-progress",
            WofCompressProgress {
                game_id: game_id.to_string(),
                algorithm: if compress {
                    algorithm.label().to_string()
                } else {
                    "Uncompressed".to_string()
                },
                files_processed: files_total,
                files_total,
                logical_bytes: stats.logical_bytes,
                compressed_bytes: stats.compressed_bytes,
                saved_bytes: stats.saved_bytes,
                done: true,
            },
        );
    }

    let compressed_count = files_changed.load(Ordering::Relaxed) as usize;
    let skipped_count = files_skipped.load(Ordering::Relaxed) as usize;
    let message = if compress {
        format!(
            "WOF {} compressed {} file(s): {} saved ({:.1}% smaller).",
            algorithm.label(),
            compressed_count,
            crate::format_bytes(stats.saved_bytes),
            stats.saved_pct
        )
    } else {
        format!("Decompressed {} file(s) back to standard NTFS allocation.", compressed_count)
    };

    WofCompressionReport {
        game_id: game_id.to_string(),
        algorithm: if compress {
            algorithm.label().to_string()
        } else {
            "Uncompressed".to_string()
        },
        logical_bytes: stats.logical_bytes,
        compressed_bytes: stats.compressed_bytes,
        saved_bytes: stats.saved_bytes,
        saved_pct: stats.saved_pct,
        files_compressed: compressed_count,
        files_skipped: skipped_count,
        elapsed_ms: started.elapsed().as_millis() as u64,
        message,
        library: Vec::new(),
    }
}

fn resolve_game_dir_for_wof(app: &AppHandle, game_id: &str) -> Result<(String, PathBuf), String> {
    let library = crate::load_library_from_disk(app)?;
    let entry = library
        .iter()
        .find(|g| g.id == game_id || g.exe_path.eq_ignore_ascii_case(game_id))
        .ok_or_else(|| "Game entry not found in library.".to_string())?;
    let dir = if !entry.install_dir.trim().is_empty() && Path::new(&entry.install_dir).is_dir() {
        PathBuf::from(&entry.install_dir)
    } else {
        crate::resolve_game_root_dir(Path::new(&entry.exe_path))
    };
    if !dir.is_dir() {
        return Err(format!("Game directory does not exist: {}", dir.display()));
    }
    Ok((entry.id.clone(), dir))
}

#[tauri::command]
pub async fn get_game_storage_stats(
    app: AppHandle,
    game_id: String,
) -> Result<GameStorageStats, String> {
    let (resolved_id, game_dir) = resolve_game_dir_for_wof(&app, &game_id)?;
    tokio::task::spawn_blocking(move || measure_game_storage_stats(&resolved_id, &game_dir))
        .await
        .map_err(|e| format!("Storage stats task failed: {e}"))
}

#[tauri::command]
pub async fn compress_game_wof(
    app: AppHandle,
    game_id: String,
    algorithm: Option<String>,
) -> Result<WofCompressionReport, String> {
    let (resolved_id, game_dir) = resolve_game_dir_for_wof(&app, &game_id)?;
    let alg = WofAlgorithm::parse(algorithm.as_deref());
    let app_clone = app.clone();
    tokio::task::spawn_blocking(move || -> Result<WofCompressionReport, String> {
        let mut report =
            run_wof_operation_sync(Some(&app_clone), &resolved_id, &game_dir, true, alg);
        let mut library = crate::load_library_from_disk(&app_clone)?;
        if let Some(entry) = library.iter_mut().find(|g| g.id == resolved_id) {
            entry.size_bytes = report.compressed_bytes;
            entry.size_formatted = crate::format_bytes(report.compressed_bytes);
        }
        crate::save_library_to_disk(&app_clone, &library)?;
        let _ = app_clone.emit("library-updated", &library);
        crate::trim_process_working_set();
        report.library = library;
        Ok(report)
    })
    .await
    .map_err(|e| format!("WOF compression worker failed: {e}"))?
}

#[tauri::command]
pub async fn decompress_game_wof(
    app: AppHandle,
    game_id: String,
) -> Result<WofCompressionReport, String> {
    let (resolved_id, game_dir) = resolve_game_dir_for_wof(&app, &game_id)?;
    let app_clone = app.clone();
    tokio::task::spawn_blocking(move || -> Result<WofCompressionReport, String> {
        let mut report = run_wof_operation_sync(
            Some(&app_clone),
            &resolved_id,
            &game_dir,
            false,
            WofAlgorithm::Lzx,
        );
        let mut library = crate::load_library_from_disk(&app_clone)?;
        if let Some(entry) = library.iter_mut().find(|g| g.id == resolved_id) {
            entry.size_bytes = report.compressed_bytes;
            entry.size_formatted = crate::format_bytes(report.compressed_bytes);
        }
        crate::save_library_to_disk(&app_clone, &library)?;
        let _ = app_clone.emit("library-updated", &library);
        crate::trim_process_working_set();
        report.library = library;
        Ok(report)
    })
    .await
    .map_err(|e| format!("WOF decompression worker failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wof_io_control_codes_and_payload_buffer_layout_match_win32_spec() {
        assert_eq!(FSCTL_SET_EXTERNAL_BACKING, 0x0009_030C);
        assert_eq!(FSCTL_DELETE_EXTERNAL_BACKING, 0x0009_0310);
        assert_eq!(FSCTL_GET_EXTERNAL_BACKING, 0x0009_0314);

        let lzx_payload = WofFileCompressionPayload::new(WofAlgorithm::Lzx);
        let bytes = lzx_payload.to_le_bytes();
        assert_eq!(bytes.len(), 20);
        // WOF_EXTERNAL_INFO { Version = 1, Provider = 2 }
        assert_eq!(&bytes[0..4], &1u32.to_le_bytes());
        assert_eq!(&bytes[4..8], &2u32.to_le_bytes());
        // FILE_PROVIDER_EXTERNAL_INFO_V1 { Version = 1, Algorithm = 1 (LZX), Flags = 0 }
        assert_eq!(&bytes[8..12], &1u32.to_le_bytes());
        assert_eq!(&bytes[12..16], &1u32.to_le_bytes());
        assert_eq!(&bytes[16..20], &0u32.to_le_bytes());

        assert_eq!(
            WofFileCompressionPayload::new(WofAlgorithm::Xpress16k)
                .file_provider_info
                .algorithm,
            3
        );
        assert_eq!(
            WofFileCompressionPayload::new(WofAlgorithm::Xpress8k)
                .file_provider_info
                .algorithm,
            2
        );
        assert_eq!(
            WofFileCompressionPayload::new(WofAlgorithm::Xpress4k)
                .file_provider_info
                .algorithm,
            0
        );
    }

    #[test]
    fn skips_tiny_files_and_already_compressed_files() {
        assert!(!should_compress_file_size(64 * 1024, 64 * 1024));
        assert!(should_compress_file_size(64 * 1024 + 1, 64 * 1024 + 1));
        assert!(!should_compress_file_size(1_000_000, 400_000));
    }
}
