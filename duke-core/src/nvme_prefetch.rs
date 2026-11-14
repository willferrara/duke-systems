//! Windows 11 NVMe Pre-Launch Cache Warmer (`PrefetchVirtualMemory` + Overlapped Read-Ahead).
//!
//! Scans the target game directory for the largest archive files (`.pak`, `.vpk`, `.bsa`, `.ba2`,
//! `.forge`, `.rpf`, `.bin`, `.dat` > 32 MiB), caps total warming to 25% of currently available
//! physical RAM (`GlobalMemoryStatusEx`) so paging never occurs, maps read-only views via
//! `CreateFileMappingW` + `MapViewOfFile`, invokes `PrefetchVirtualMemory`, and issues
//! parallel `FILE_FLAG_OVERLAPPED` read-ahead across worker threads to populate Standby RAM.

use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

pub const DEFAULT_MIN_ARCHIVE_BYTES: u64 = 32 * 1024 * 1024;
const PREFETCH_EXTENSIONS: &[&str] = &["pak", "vpk", "bsa", "ba2", "forge", "rpf", "bin", "dat"];
const MAX_VIEW_WINDOW_BYTES: usize = 64 * 1024 * 1024;
const PREFETCH_BATCH_STRIDE_BYTES: usize = 4 * 1024 * 1024;
const OVERLAPPED_READ_CHUNK_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefetchFileStat {
    pub path: String,
    pub file_size_bytes: u64,
    pub warmed_bytes: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefetchReport {
    pub warmed_bytes: u64,
    pub warmed_mb: f64,
    pub throughput_gbps: f64,
    pub elapsed_ms: u64,
    pub files_warmed: usize,
    pub budget_bytes: u64,
    pub available_phys_mb: u64,
    pub used_prefetch_api: bool,
    pub files: Vec<PrefetchFileStat>,
}

#[cfg(target_os = "windows")]
#[repr(C)]
#[derive(Clone, Copy)]
struct Win32MemoryRangeEntry {
    virtual_address: *mut core::ffi::c_void,
    number_of_bytes: usize,
}

#[cfg(target_os = "windows")]
type PrefetchVirtualMemoryFn = unsafe extern "system" fn(
    h_process: windows::Win32::Foundation::HANDLE,
    number_of_entries: usize,
    virtual_addresses: *const Win32MemoryRangeEntry,
    flags: u32,
) -> i32;

#[cfg(target_os = "windows")]
static PREFETCH_VIRTUAL_MEMORY_PROC: OnceLock<Option<PrefetchVirtualMemoryFn>> = OnceLock::new();

#[cfg(target_os = "windows")]
fn get_prefetch_virtual_memory_fn() -> Option<PrefetchVirtualMemoryFn> {
    *PREFETCH_VIRTUAL_MEMORY_PROC.get_or_init(|| {
        use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
        use windows::core::s;

        // SAFETY: `kernel32.dll` is loaded in every Win32 process; `GetProcAddress` signature matches Windows SDK `PrefetchVirtualMemory`.
        unsafe {
            let h_mod = GetModuleHandleA(s!("kernel32.dll")).ok()?;
            let proc_addr = GetProcAddress(h_mod, s!("PrefetchVirtualMemory"))?;
            Some(std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                PrefetchVirtualMemoryFn,
            >(proc_addr))
        }
    })
}

/// Queries currently available physical RAM bytes via Win32 `GlobalMemoryStatusEx`.
#[cfg(target_os = "windows")]
pub fn query_available_physical_ram_bytes() -> u64 {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: `dwLength` initialized to `size_of::<MEMORYSTATUSEX>()`.
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) }.is_ok();
    if ok {
        status.ullAvailPhys
    } else {
        512 * 1024 * 1024
    }
}

#[cfg(not(target_os = "windows"))]
pub fn query_available_physical_ram_bytes() -> u64 {
    512 * 1024 * 1024
}

fn is_candidate_archive_path(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .map(|ext| PREFETCH_EXTENSIONS.iter().any(|cand| ext.eq_ignore_ascii_case(cand)))
        .unwrap_or(false)
}

fn collect_archive_candidates(root: &Path, min_file_bytes: u64) -> Vec<(PathBuf, u64)> {
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut found = Vec::new();
    const MAX_DEPTH: usize = 6;

    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.eq_ignore_ascii_case(".duke-cas")
                || name_str.eq_ignore_ascii_case(".duke-profile")
                || name_str.eq_ignore_ascii_case(".git")
            {
                continue;
            }
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if ft.is_dir() && depth < MAX_DEPTH {
                stack.push((path, depth + 1));
            } else if ft.is_file()
                && is_candidate_archive_path(&path)
                && let Ok(meta) = entry.metadata()
            {
                let len = meta.len();
                if len > min_file_bytes {
                    found.push((path, len));
                }
            }
        }
    }

    // Prioritize the largest archive assets first so key `.pak`/`.vpk`/`.ba2` containers warm first.
    found.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    found
}

/// Computes bounded prefetch budget (`budget_pct` clamped `0..=40%` of available physical RAM)
/// and assigns target byte caps.
fn plan_prefetch_slices_with_pct(
    candidates: &[(PathBuf, u64)],
    avail_phys_bytes: u64,
    budget_pct: u8,
) -> (u64, Vec<(PathBuf, u64, u64)>) {
    let clamped_pct = budget_pct.min(40) as u64;
    if clamped_pct == 0 {
        return (0, Vec::new());
    }
    let budget = avail_phys_bytes.saturating_mul(clamped_pct) / 100;
    if budget == 0 {
        return (0, Vec::new());
    }
    let mut remaining = budget;
    let mut plan = Vec::new();
    for (path, size_bytes) in candidates {
        if remaining == 0 {
            break;
        }
        let target = (*size_bytes).min(remaining);
        if target > 0 {
            plan.push((path.clone(), *size_bytes, target));
            remaining = remaining.saturating_sub(target);
        }
    }
    (budget, plan)
}

#[cfg(test)]
/// Computes bounded prefetch budget (`<= 25%` of available physical RAM) and assigns target byte caps.
fn plan_prefetch_slices(candidates: &[(PathBuf, u64)], avail_phys_bytes: u64) -> (u64, Vec<(PathBuf, u64, u64)>) {
    plan_prefetch_slices_with_pct(candidates, avail_phys_bytes, 25)
}

#[cfg(target_os = "windows")]
fn warm_single_file_windows(path: &Path, target_bytes: u64) -> Result<(u64, bool), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::{CloseHandle, ERROR_IO_PENDING, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_OVERLAPPED, FILE_FLAG_SEQUENTIAL_SCAN,
        FILE_GENERIC_READ, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        ReadFile,
    };
    use windows::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};
    use windows::Win32::System::Memory::{
        CreateFileMappingW, FILE_MAP_READ, MapViewOfFile, PAGE_READONLY, UnmapViewOfFile,
    };
    use windows::Win32::System::Threading::{
        CreateEventW, GetCurrentProcess, ResetEvent, WaitForSingleObject,
    };
    use windows::core::{HRESULT, PCWSTR};

    if target_bytes == 0 {
        return Ok((0, false));
    }

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // 1. Open file handle for section mapping (`CreateFileMappingW` works cleanly on standard sequential handle).
    // SAFETY: Valid null-terminated wide path, read-only sharing allowed.
    let map_file_handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_SEQUENTIAL_SCAN,
            None,
        )
    }
    .map_err(|e| format!("CreateFileW('{}') failed: {}", path.display(), e))?;

    let mut used_prefetch = false;
    let prefetch_fn = get_prefetch_virtual_memory_fn();

    // 2. Map bounded read-only views (64 MiB alignment-safe windows) & invoke `PrefetchVirtualMemory`.
    // SAFETY: Creating read-only section object over opened file handle.
    let mapping_res = unsafe { CreateFileMappingW(map_file_handle, None, PAGE_READONLY, 0, 0, None) };
    if let Ok(mapping) = mapping_res
        && !mapping.is_invalid()
    {
        let cur_proc = unsafe { GetCurrentProcess() };
            let mut offset = 0u64;
            while offset < target_bytes {
                let win_len = (target_bytes - offset).min(MAX_VIEW_WINDOW_BYTES as u64) as usize;
                let off_high = (offset >> 32) as u32;
                let off_low = (offset & 0xFFFF_FFFF) as u32;

                // SAFETY: `offset` is a multiple of `MAX_VIEW_WINDOW_BYTES` (64 MiB), satisfying Win32 64 KiB allocation granularity.
                let view = unsafe { MapViewOfFile(mapping, FILE_MAP_READ, off_high, off_low, win_len) };
                if !view.Value.is_null() {
                    if let Some(pf) = prefetch_fn {
                        let mut ranges = Vec::with_capacity(
                            (win_len / PREFETCH_BATCH_STRIDE_BYTES).saturating_add(1),
                        );
                        let mut local_off = 0usize;
                        while local_off < win_len {
                            let chunk = (win_len - local_off).min(PREFETCH_BATCH_STRIDE_BYTES);
                            ranges.push(Win32MemoryRangeEntry {
                                // SAFETY: Pointer arithmetic stays strictly inside mapped view `[0, win_len)`.
                                virtual_address: unsafe { (view.Value as *mut u8).add(local_off) }
                                    as *mut core::ffi::c_void,
                                number_of_bytes: chunk,
                            });
                            local_off += chunk;
                        }
                        if !ranges.is_empty() {
                            // SAFETY: Passing valid `WIN32_MEMORY_RANGE_ENTRY` batch covering mapped view.
                            let ok = unsafe { pf(cur_proc, ranges.len(), ranges.as_ptr(), 0) };
                            if ok != 0 {
                                used_prefetch = true;
                            }
                        }
                    }
                    // SAFETY: Unmap immediately so virtual address space is freed while Standby pages remain resident.
                    unsafe {
                        let _ = UnmapViewOfFile(view);
                    }
                }
                offset = offset.saturating_add(win_len as u64);
            }
        // SAFETY: Closing section handle.
        unsafe {
            let _ = CloseHandle(mapping);
        }
    }
    // SAFETY: Closing sequential file handle.
    unsafe {
        let _ = CloseHandle(map_file_handle);
    }

    // 3. Pair with parallel overlapped read-ahead (`FILE_FLAG_OVERLAPPED`) using two pipelined 1 MiB buffers.
    // SAFETY: Valid wide path opened with `FILE_FLAG_OVERLAPPED` for async NVMe read-ahead.
    let async_handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED | FILE_FLAG_SEQUENTIAL_SCAN,
            None,
        )
    }
    .map_err(|e| format!("Overlapped CreateFileW('{}') failed: {}", path.display(), e))?;

    let mut ev_a = HANDLE::default();
    let mut ev_b = HANDLE::default();
    // SAFETY: Creating manual-reset events for double-buffered overlapped NVMe reads.
    if let Ok(h) = unsafe { CreateEventW(None, true, false, None) } {
        ev_a = h;
    }
    if let Ok(h) = unsafe { CreateEventW(None, true, false, None) } {
        ev_b = h;
    }

    let mut warmed_total = 0u64;
    let mut buf_a = vec![0u8; OVERLAPPED_READ_CHUNK_BYTES];
    let mut buf_b = vec![0u8; OVERLAPPED_READ_CHUNK_BYTES];
    let mut next_off = 0u64;

    if !ev_a.is_invalid() && !ev_b.is_invalid() {
        let mut ov = [OVERLAPPED::default(), OVERLAPPED::default()];
        let mut active = [false, false];
        let mut requested = [0usize, 0usize];
        let events = [ev_a, ev_b];
        let bufs: [*mut u8; 2] = [buf_a.as_mut_ptr(), buf_b.as_mut_ptr()];

        let issue_read = |slot: usize,
                          off: u64,
                          ov_slot: &mut OVERLAPPED,
                          req_len: &mut usize,
                          is_active: &mut bool| {
            let chunk = (target_bytes.saturating_sub(off)).min(OVERLAPPED_READ_CHUNK_BYTES as u64)
                as usize;
            if chunk == 0 {
                return false;
            }
            *ov_slot = OVERLAPPED::default();
            ov_slot.Anonymous.Anonymous.Offset = (off & 0xFFFF_FFFF) as u32;
            ov_slot.Anonymous.Anonymous.OffsetHigh = (off >> 32) as u32;
            ov_slot.hEvent = events[slot];
            // SAFETY: Resetting manual-reset event before issuing new overlapped ReadFile.
            let _ = unsafe { ResetEvent(events[slot]) };

            // SAFETY: Buffer slot stays alive and untouched until `GetOverlappedResult` finishes.
            let slice = unsafe { std::slice::from_raw_parts_mut(bufs[slot], chunk) };
            let res = unsafe {
                ReadFile(
                    async_handle,
                    Some(slice),
                    None,
                    Some(ov_slot as *mut OVERLAPPED),
                )
            };
            let io_pending_hr = HRESULT::from_win32(ERROR_IO_PENDING.0);
            match res {
                Ok(()) => {
                    *req_len = chunk;
                    *is_active = true;
                    true
                }
                Err(err) if err.code() == io_pending_hr => {
                    *req_len = chunk;
                    *is_active = true;
                    true
                }
                Err(_) => false,
            }
        };

        // Prime initial 2 in-flight overlapped reads
        for slot in 0..2 {
            if next_off < target_bytes
                && issue_read(slot, next_off, &mut ov[slot], &mut requested[slot], &mut active[slot])
            {
                next_off += requested[slot] as u64;
            }
        }

        let mut slot = 0usize;
        while active[0] || active[1] {
            if active[slot] {
                let mut completed = 0u32;
                // SAFETY: Waiting on valid `OVERLAPPED` event paired with active `ReadFile`.
                let wait = unsafe { WaitForSingleObject(events[slot], 5000) };
                if wait == WAIT_OBJECT_0 {
                    // SAFETY: Retrieving result for completed async read.
                    if unsafe { GetOverlappedResult(async_handle, &ov[slot], &mut completed, true) }
                        .is_ok()
                    {
                        warmed_total = warmed_total.saturating_add(completed as u64);
                    }
                }
                active[slot] = false;
                if next_off < target_bytes
                    && completed > 0
                    && issue_read(
                        slot,
                        next_off,
                        &mut ov[slot],
                        &mut requested[slot],
                        &mut active[slot],
                    )
                {
                    next_off += requested[slot] as u64;
                }
            }
            slot = 1 - slot;
        }
    }

    // SAFETY: Close async file and event handles.
    unsafe {
        if !ev_a.is_invalid() {
            let _ = CloseHandle(ev_a);
        }
        if !ev_b.is_invalid() {
            let _ = CloseHandle(ev_b);
        }
        let _ = CloseHandle(async_handle);
    }

    Ok((warmed_total, used_prefetch))
}

#[cfg(not(target_os = "windows"))]
fn warm_single_file_windows(path: &Path, target_bytes: u64) -> Result<(u64, bool), String> {
    use std::io::Read;
    let mut file = fs::File::open(path).map_err(|e| format!("Failed opening {}: {}", path.display(), e))?;
    let mut buf = vec![0u8; OVERLAPPED_READ_CHUNK_BYTES];
    let mut remaining = target_bytes;
    let mut total = 0u64;
    while remaining > 0 {
        let to_read = (remaining as usize).min(buf.len());
        let n = file.read(&mut buf[..to_read]).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        total += n as u64;
        remaining -= n as u64;
    }
    Ok((total, false))
}

/// Scans `game_dir` for large game archives (> 32 MiB) capped at 25% of available physical RAM
/// and warms Windows Standby RAM cache across parallel worker threads.
pub fn warm_game_assets(game_dir: &Path) -> Result<PrefetchReport, String> {
    warm_game_assets_with_options(game_dir, DEFAULT_MIN_ARCHIVE_BYTES, 25)
}

/// Scans `game_dir` for large game archives (> 32 MiB) with a configurable RAM budget percentage (`0..=40`).
pub fn warm_game_assets_with_budget_pct(game_dir: &Path, budget_pct: u8) -> Result<PrefetchReport, String> {
    warm_game_assets_with_options(game_dir, DEFAULT_MIN_ARCHIVE_BYTES, budget_pct)
}

/// Configurable variant allowing unit tests to validate full Win32 `PrefetchVirtualMemory`
/// and `GlobalMemoryStatusEx` behavior without writing multi-32 MiB fixtures to disk.
pub fn warm_game_assets_with_min_size(
    game_dir: &Path,
    min_file_bytes: u64,
) -> Result<PrefetchReport, String> {
    warm_game_assets_with_options(game_dir, min_file_bytes, 25)
}

/// Configurable Standby RAM prefetcher with explicit file threshold and percentage budget (`0..=40`).
pub fn warm_game_assets_with_options(
    game_dir: &Path,
    min_file_bytes: u64,
    budget_pct: u8,
) -> Result<PrefetchReport, String> {
    if budget_pct == 0 || !game_dir.exists() || !game_dir.is_dir() {
        return Ok(PrefetchReport::default());
    }

    let started = Instant::now();
    let avail_phys = query_available_physical_ram_bytes();
    let candidates = collect_archive_candidates(game_dir, min_file_bytes);
    let (budget_bytes, plan) = plan_prefetch_slices_with_pct(&candidates, avail_phys, budget_pct);

    if plan.is_empty() {
        return Ok(PrefetchReport {
            warmed_bytes: 0,
            warmed_mb: 0.0,
            throughput_gbps: 0.0,
            elapsed_ms: started.elapsed().as_millis() as u64,
            files_warmed: 0,
            budget_bytes,
            available_phys_mb: avail_phys / (1024 * 1024),
            used_prefetch_api: false,
            files: Vec::new(),
        });
    }

    let total_warmed = AtomicU64::new(0);
    let used_api = AtomicBool::new(false);
    let next_idx = AtomicU64::new(0);
    let file_stats = std::sync::Mutex::new(Vec::<PrefetchFileStat>::with_capacity(plan.len()));

    let worker_count = std::thread::available_parallelism()
        .map(|n| n.get().clamp(2, 6))
        .unwrap_or(4)
        .min(plan.len());

    std::thread::scope(|s| {
        for _ in 0..worker_count {
            let total_ref = &total_warmed;
            let used_ref = &used_api;
            let idx_ref = &next_idx;
            let stats_ref = &file_stats;
            let plan_ref = &plan;

            s.spawn(move || loop {
                let i = idx_ref.fetch_add(1, Ordering::Relaxed) as usize;
                if i >= plan_ref.len() {
                    break;
                }
                let (ref path, size_bytes, target_bytes) = plan_ref[i];
                if let Ok((warmed, prefetch_ok)) = warm_single_file_windows(path, target_bytes) {
                    total_ref.fetch_add(warmed, Ordering::Relaxed);
                    if prefetch_ok {
                        used_ref.store(true, Ordering::Relaxed);
                    }
                    if let Ok(mut guard) = stats_ref.lock() {
                        guard.push(PrefetchFileStat {
                            path: path.to_string_lossy().to_string(),
                            file_size_bytes: size_bytes,
                            warmed_bytes: warmed,
                        });
                    }
                }
            });
        }
    });

    let elapsed = started.elapsed();
    let elapsed_secs = elapsed.as_secs_f64().max(0.000_1);
    let warmed_bytes = total_warmed.load(Ordering::Relaxed);
    let warmed_mb = (warmed_bytes as f64 / (1024.0 * 1024.0) * 10.0).round() / 10.0;
    let throughput_gbps = ((warmed_bytes as f64 / 1_000_000_000.0) / elapsed_secs * 100.0).round() / 100.0;

    let files = file_stats.into_inner().unwrap_or_default();

    Ok(PrefetchReport {
        warmed_bytes,
        warmed_mb,
        throughput_gbps,
        elapsed_ms: elapsed.as_millis() as u64,
        files_warmed: files.len(),
        budget_bytes,
        available_phys_mb: avail_phys / (1024 * 1024),
        used_prefetch_api: used_api.load(Ordering::Relaxed),
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforces_25_percent_ram_budget_and_largest_archive_priority() {
        let candidates = vec![
            (PathBuf::from("game\\pakchunk0.pak"), 800 * 1024 * 1024),
            (PathBuf::from("game\\pakchunk1.pak"), 500 * 1024 * 1024),
            (PathBuf::from("game\\small.pak"), 100 * 1024 * 1024),
        ];
        // Suppose 4 GiB available physical RAM -> 25% cap = 1 GiB (1024 MiB)
        let avail = 4 * 1024 * 1024 * 1024;
        let (budget, plan) = plan_prefetch_slices(&candidates, avail);
        assert_eq!(budget, 1024 * 1024 * 1024);
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].2, 800 * 1024 * 1024);
        assert_eq!(plan[1].2, 224 * 1024 * 1024);
        let sum: u64 = plan.iter().map(|(_, _, t)| *t).sum();
        assert_eq!(sum, budget);
    }

    #[test]
    fn filters_extensions_and_warms_real_archives_via_win32_prefetch() {
        let dir = std::env::temp_dir().join(format!("duke_nvme_prefetch_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let pak_path = dir.join("world_assets.pak");
        let vpk_path = dir.join("audio_stream.vpk");
        let txt_path = dir.join("readme.txt");

        let payload_pak = vec![0x5Au8; 512 * 1024];
        let payload_vpk = vec![0x3Cu8; 256 * 1024];
        fs::write(&pak_path, &payload_pak).unwrap();
        fs::write(&vpk_path, &payload_vpk).unwrap();
        fs::write(&txt_path, &payload_pak).unwrap();

        // Production 32 MiB threshold skips small files cleanly
        let prod_report = warm_game_assets(&dir).unwrap();
        assert_eq!(prod_report.files_warmed, 0);

        // 0% budget cleanly disables pre-launch caching
        let off_report = warm_game_assets_with_options(&dir, 64 * 1024, 0).unwrap();
        assert_eq!(off_report.files_warmed, 0);
        assert_eq!(off_report.budget_bytes, 0);

        // Custom percentage slice bounds (clamped at 40%)
        let candidates = vec![(PathBuf::from("game\\pak0.pak"), 1024 * 1024 * 1024)];
        let (b10, _) = plan_prefetch_slices_with_pct(&candidates, 1000, 10);
        let (b40, _) = plan_prefetch_slices_with_pct(&candidates, 1000, 99);
        assert_eq!(b10, 100);
        assert_eq!(b40, 400);

        // Lower threshold exercises full CreateFileMappingW + PrefetchVirtualMemory + Overlapped path
        let report = warm_game_assets_with_min_size(&dir, 64 * 1024).unwrap();
        assert_eq!(report.files_warmed, 2);
        assert_eq!(report.warmed_bytes, (512 + 256) * 1024);
        assert!(report.available_phys_mb > 0);
        assert!(report.throughput_gbps > 0.0);
        #[cfg(target_os = "windows")]
        assert!(report.used_prefetch_api);

        let _ = fs::remove_dir_all(&dir);
    }
}
