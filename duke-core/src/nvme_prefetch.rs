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
const PREFETCH_EXTENSIONS: &[&str] = &[
    "pak", "vpk", "bsa", "ba2", "forge", "rpf", "bin", "dat",
    // Console ROM / disc images (> 32 MiB) for emulator Standby RAM warming
    "rvz", "chd", "iso", "xci", "nsp", "wua", "wux", "cso", "pbp", "gcm", "wbfs", "cci", "3ds", "nds",
];
const MAX_VIEW_WINDOW_BYTES: usize = 64 * 1024 * 1024;
const PREFETCH_BATCH_STRIDE_BYTES: usize = 4 * 1024 * 1024;
const OVERLAPPED_READ_CHUNK_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IoRingCapability {
    pub supported: bool,
    pub windows_build: u32,
    pub min_required_build: u32,
    pub apis_resolved: bool,
    pub backend: String,
}

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
    #[serde(default)]
    pub used_ioring_api: bool,
    #[serde(default)]
    pub windows_build: u32,
    pub files: Vec<PrefetchFileStat>,
}

const IORING_MIN_WIN11_BUILD: u32 = 22621;
const IORING_BATCH_SLOTS: usize = 16;
const IORING_SLOT_BYTES: usize = 512 * 1024;

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
#[repr(C)]
struct RtlOsVersionInfoExW {
    os_version_info_size: u32,
    major_version: u32,
    minor_version: u32,
    build_number: u32,
    platform_id: u32,
    sz_csd_version: [u16; 128],
    service_pack_major: u16,
    service_pack_minor: u16,
    suite_mask: u16,
    product_type: u8,
    reserved: u8,
}

#[cfg(target_os = "windows")]
type RtlGetVersionFn = unsafe extern "system" fn(*mut RtlOsVersionInfoExW) -> i32;

#[cfg(target_os = "windows")]
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct IoringCreateFlags {
    required: u32,
    advisory: u32,
}

#[cfg(target_os = "windows")]
#[repr(C)]
#[derive(Clone, Copy)]
struct IoringHandleRef {
    kind: u32,
    handle: isize,
}

#[cfg(target_os = "windows")]
#[repr(C)]
#[derive(Clone, Copy)]
struct IoringBufferRef {
    kind: u32,
    address: *mut core::ffi::c_void,
}

#[cfg(target_os = "windows")]
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct IoringCqe {
    user_data: usize,
    result_code: i32,
    information: usize,
}

#[cfg(target_os = "windows")]
type CreateIoRingFn = unsafe extern "system" fn(
    ioring_version: u32,
    flags: IoringCreateFlags,
    submission_queue_size: u32,
    completion_queue_size: u32,
    h: *mut *mut core::ffi::c_void,
) -> i32;

#[cfg(target_os = "windows")]
type BuildIoRingReadFileFn = unsafe extern "system" fn(
    io_ring: *mut core::ffi::c_void,
    file_ref: IoringHandleRef,
    data_ref: IoringBufferRef,
    number_of_bytes_to_read: u32,
    file_offset: u64,
    user_data: usize,
    sqe_flags: u32,
) -> i32;

#[cfg(target_os = "windows")]
type BuildIoRingWriteFileFn = unsafe extern "system" fn(
    io_ring: *mut core::ffi::c_void,
    file_ref: IoringHandleRef,
    data_ref: IoringBufferRef,
    number_of_bytes_to_write: u32,
    file_offset: u64,
    write_flags: u32,
    user_data: usize,
    sqe_flags: u32,
) -> i32;

#[cfg(target_os = "windows")]
type SubmitIoRingFn = unsafe extern "system" fn(
    io_ring: *mut core::ffi::c_void,
    wait_operations: u32,
    milliseconds: u32,
    submitted_entries: *mut u32,
) -> i32;

#[cfg(target_os = "windows")]
type PopIoRingCompletionFn =
    unsafe extern "system" fn(io_ring: *mut core::ffi::c_void, cqe: *mut IoringCqe) -> i32;

#[cfg(target_os = "windows")]
type CloseIoRingFn = unsafe extern "system" fn(io_ring: *mut core::ffi::c_void) -> i32;

#[cfg(target_os = "windows")]
#[derive(Clone, Copy)]
struct IoRingProcs {
    create_io_ring: CreateIoRingFn,
    build_read_file: BuildIoRingReadFileFn,
    build_write_file: Option<BuildIoRingWriteFileFn>,
    submit_io_ring: SubmitIoRingFn,
    pop_completion: PopIoRingCompletionFn,
    close_io_ring: CloseIoRingFn,
}

#[cfg(target_os = "windows")]
static IORING_PROCS: OnceLock<Option<IoRingProcs>> = OnceLock::new();
#[cfg(target_os = "windows")]
static WINDOWS_BUILD_CACHE: OnceLock<u32> = OnceLock::new();

#[cfg(target_os = "windows")]
fn get_ioring_procs() -> Option<IoRingProcs> {
    *IORING_PROCS.get_or_init(|| {
        use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress, LoadLibraryA};
        use windows::core::s;

        unsafe {
            let hmod = GetModuleHandleA(s!("kernelbase.dll"))
                .or_else(|_| LoadLibraryA(s!("kernelbase.dll")))
                .ok()?;
            let create = GetProcAddress(hmod, s!("CreateIoRing"))?;
            let build = GetProcAddress(hmod, s!("BuildIoRingReadFile"))?;
            let build_write = GetProcAddress(hmod, s!("BuildIoRingWriteFile")).map(|p| {
                std::mem::transmute::<unsafe extern "system" fn() -> isize, BuildIoRingWriteFileFn>(
                    p,
                )
            });
            let submit = GetProcAddress(hmod, s!("SubmitIoRing"))?;
            let pop = GetProcAddress(hmod, s!("PopIoRingCompletion"))?;
            let close = GetProcAddress(hmod, s!("CloseIoRing"))?;
            Some(IoRingProcs {
                create_io_ring: std::mem::transmute::<unsafe extern "system" fn() -> isize, CreateIoRingFn>(create),
                build_read_file: std::mem::transmute::<unsafe extern "system" fn() -> isize, BuildIoRingReadFileFn>(build),
                build_write_file: build_write,
                submit_io_ring: std::mem::transmute::<unsafe extern "system" fn() -> isize, SubmitIoRingFn>(submit),
                pop_completion: std::mem::transmute::<unsafe extern "system" fn() -> isize, PopIoRingCompletionFn>(pop),
                close_io_ring: std::mem::transmute::<unsafe extern "system" fn() -> isize, CloseIoRingFn>(close),
            })
        }
    })
}

/// Returns the true Windows OS build number via `ntdll!RtlGetVersion`.
pub fn windows_build_number() -> u32 {
    #[cfg(target_os = "windows")]
    {
        *WINDOWS_BUILD_CACHE.get_or_init(|| {
            use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
            use windows::core::s;

            unsafe {
                let Ok(hmod) = GetModuleHandleA(s!("ntdll.dll")) else {
                    return 0;
                };
                let Some(proc_addr) = GetProcAddress(hmod, s!("RtlGetVersion")) else {
                    return 0;
                };
                let rtl_ver = std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    RtlGetVersionFn,
                >(proc_addr);
                let mut info: RtlOsVersionInfoExW = std::mem::zeroed();
                info.os_version_info_size = std::mem::size_of::<RtlOsVersionInfoExW>() as u32;
                if rtl_ver(&mut info) >= 0 {
                    info.build_number
                } else {
                    0
                }
            }
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        0
    }
}

/// Probes whether Windows 11 22H2+ Kernel `IoRing` (`CreateIoRing`) is supported and resolved.
pub fn probe_ioring_support() -> IoRingCapability {
    let build = windows_build_number();
    #[cfg(target_os = "windows")]
    let resolved = get_ioring_procs().is_some();
    #[cfg(not(target_os = "windows"))]
    let resolved = false;
    let supported = build >= IORING_MIN_WIN11_BUILD && resolved;
    IoRingCapability {
        supported,
        windows_build: build,
        min_required_build: IORING_MIN_WIN11_BUILD,
        apis_resolved: resolved,
        backend: if supported {
            format!("Win11 Kernel IoRing (Build {})", build)
        } else {
            format!("PrefetchVirtualMemory + Overlapped Fallback (Build {})", build)
        },
    }
}

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
fn warm_file_via_ioring(
    wide_path: &[u16],
    target_bytes: u64,
    procs: &IoRingProcs,
) -> Result<u64, String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_SEQUENTIAL_SCAN, FILE_GENERIC_READ,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::core::PCWSTR;

    if target_bytes == 0 {
        return Ok(0);
    }

    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide_path.as_ptr()),
            FILE_GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_SEQUENTIAL_SCAN,
            None,
        )
    }
    .map_err(|e| format!("IoRing CreateFileW failed: {}", e))?;

    let mut ring: *mut core::ffi::c_void = std::ptr::null_mut();
    let flags = IoringCreateFlags::default();
    let mut create_hr = -1i32;
    for ver in [3u32, 2u32, 1u32] {
        create_hr = unsafe { (procs.create_io_ring)(ver, flags, 64, 64, &mut ring) };
        if create_hr >= 0 && !ring.is_null() {
            break;
        }
    }
    if create_hr < 0 || ring.is_null() {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return Err(format!("CreateIoRing failed: 0x{:08X}", create_hr as u32));
    }

    let mut slot_buffers: Vec<Vec<u8>> = (0..IORING_BATCH_SLOTS)
        .map(|_| vec![0u8; IORING_SLOT_BYTES])
        .collect();
    let mut offset = 0u64;
    let mut total_read = 0u64;
    let file_ref = IoringHandleRef {
        kind: 0,
        handle: handle.0 as isize,
    };

    while offset < target_bytes {
        let mut queued = 0u32;
        for (slot_idx, buf) in slot_buffers.iter_mut().enumerate() {
            if offset >= target_bytes {
                break;
            }
            let chunk = (target_bytes - offset).min(IORING_SLOT_BYTES as u64) as u32;
            if chunk == 0 {
                break;
            }
            let data_ref = IoringBufferRef {
                kind: 0,
                address: buf.as_mut_ptr().cast::<core::ffi::c_void>(),
            };
            let hr = unsafe {
                (procs.build_read_file)(ring, file_ref, data_ref, chunk, offset, slot_idx, 0)
            };
            if hr < 0 {
                break;
            }
            offset = offset.saturating_add(chunk as u64);
            queued += 1;
        }
        if queued == 0 {
            break;
        }
        let mut submitted = 0u32;
        let submit_hr = unsafe { (procs.submit_io_ring)(ring, queued, 15_000, &mut submitted) };
        if submit_hr < 0 {
            unsafe {
                let _ = (procs.close_io_ring)(ring);
                let _ = CloseHandle(handle);
            }
            return Err(format!("SubmitIoRing failed: 0x{:08X}", submit_hr as u32));
        }

        let mut drained = 0u32;
        while drained < queued {
            let mut cqe = IoringCqe::default();
            let pop_hr = unsafe { (procs.pop_completion)(ring, &mut cqe) };
            if pop_hr != 0 {
                break;
            }
            drained += 1;
            if cqe.result_code >= 0 {
                total_read = total_read.saturating_add(cqe.information as u64);
            }
        }
    }

    unsafe {
        let _ = (procs.close_io_ring)(ring);
        let _ = CloseHandle(handle);
    }
    Ok(total_read)
}

#[cfg(target_os = "windows")]
fn warm_single_file_windows(path: &Path, target_bytes: u64) -> Result<(u64, bool, bool), String> {
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
        return Ok((0, false, false));
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

    // 3. On Win11 22H2+ (build >= 22621), submit batched kernel IoRing reads; otherwise fall back to double-buffered overlapped ReadFile.
    let cap = probe_ioring_support();
    if cap.supported
        && let Some(procs) = get_ioring_procs()
        && let Ok(ioring_warmed) = warm_file_via_ioring(&wide, target_bytes, &procs)
        && ioring_warmed > 0
    {
        return Ok((ioring_warmed, used_prefetch, true));
    }

    // Fallback: Pair with parallel overlapped read-ahead (`FILE_FLAG_OVERLAPPED`) using two pipelined 1 MiB buffers.
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

    Ok((warmed_total, used_prefetch, false))
}

#[cfg(not(target_os = "windows"))]
fn warm_single_file_windows(path: &Path, target_bytes: u64) -> Result<(u64, bool, bool), String> {
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
    Ok((total, false, false))
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
    let win_build = windows_build_number();
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
            used_ioring_api: false,
            windows_build: win_build,
            files: Vec::new(),
        });
    }

    let total_warmed = AtomicU64::new(0);
    let used_api = AtomicBool::new(false);
    let used_ioring = AtomicBool::new(false);
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
            let ioring_ref = &used_ioring;
            let idx_ref = &next_idx;
            let stats_ref = &file_stats;
            let plan_ref = &plan;

            s.spawn(move || loop {
                let i = idx_ref.fetch_add(1, Ordering::Relaxed) as usize;
                if i >= plan_ref.len() {
                    break;
                }
                let (ref path, size_bytes, target_bytes) = plan_ref[i];
                if let Ok((warmed, prefetch_ok, ioring_ok)) = warm_single_file_windows(path, target_bytes) {
                    total_ref.fetch_add(warmed, Ordering::Relaxed);
                    if prefetch_ok {
                        used_ref.store(true, Ordering::Relaxed);
                    }
                    if ioring_ok {
                        ioring_ref.store(true, Ordering::Relaxed);
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
        used_ioring_api: used_ioring.load(Ordering::Relaxed),
        windows_build: win_build,
        files,
    })
}

/// Warms a single game archive or console ROM/ISO image (> 32 MiB) into Windows Standby RAM
/// up to `budget_pct` (`0..=40`) of currently available physical RAM.
pub fn warm_single_file_with_budget_pct(
    file_path: &Path,
    budget_pct: u8,
) -> Result<PrefetchReport, String> {
    warm_single_file_with_options(file_path, DEFAULT_MIN_ARCHIVE_BYTES, budget_pct)
}

/// Configurable single-file Standby RAM prefetcher respecting minimum size and RAM percentage cap.
pub fn warm_single_file_with_options(
    file_path: &Path,
    min_file_bytes: u64,
    budget_pct: u8,
) -> Result<PrefetchReport, String> {
    if budget_pct == 0 || !file_path.exists() || !file_path.is_file() {
        return Ok(PrefetchReport::default());
    }

    let started = Instant::now();
    let avail_phys = query_available_physical_ram_bytes();
    let win_build = windows_build_number();
    let size_bytes = fs::metadata(file_path).map(|m| m.len()).unwrap_or(0);
    if size_bytes < min_file_bytes {
        return Ok(PrefetchReport {
            warmed_bytes: 0,
            warmed_mb: 0.0,
            throughput_gbps: 0.0,
            elapsed_ms: started.elapsed().as_millis() as u64,
            files_warmed: 0,
            budget_bytes: (avail_phys.saturating_mul(budget_pct.clamp(0, 40) as u64)) / 100,
            available_phys_mb: avail_phys / (1024 * 1024),
            used_prefetch_api: false,
            used_ioring_api: false,
            windows_build: win_build,
            files: Vec::new(),
        });
    }

    let candidates = vec![(file_path.to_path_buf(), size_bytes)];
    let (budget_bytes, plan) = plan_prefetch_slices_with_pct(&candidates, avail_phys, budget_pct);
    if plan.is_empty() {
        return Ok(PrefetchReport::default());
    }

    let (_, total_size, target_bytes) = plan[0];
    let (warmed_bytes, used_prefetch_api, used_ioring_api) = warm_single_file_windows(file_path, target_bytes)?;
    let elapsed = started.elapsed();
    let elapsed_secs = elapsed.as_secs_f64().max(0.000_1);
    let warmed_mb = (warmed_bytes as f64 / (1024.0 * 1024.0) * 10.0).round() / 10.0;
    let throughput_gbps =
        ((warmed_bytes as f64 / 1_000_000_000.0) / elapsed_secs * 100.0).round() / 100.0;

    Ok(PrefetchReport {
        warmed_bytes,
        warmed_mb,
        throughput_gbps,
        elapsed_ms: elapsed.as_millis() as u64,
        files_warmed: usize::from(warmed_bytes > 0),
        budget_bytes,
        available_phys_mb: avail_phys / (1024 * 1024),
        used_prefetch_api,
        used_ioring_api,
        windows_build: win_build,
        files: vec![PrefetchFileStat {
            path: file_path.to_string_lossy().to_string(),
            file_size_bytes: total_size,
            warmed_bytes,
        }],
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IoRingTransferReport {
    pub bytes_transferred: u64,
    pub used_ioring_api: bool,
    pub elapsed_ms: u64,
    pub backend: String,
}

#[cfg(target_os = "windows")]
fn try_ioring_copy_file_windows(src: &Path, dst: &Path, size_bytes: u64) -> Result<u64, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Storage::FileSystem::{
        CREATE_ALWAYS, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_SEQUENTIAL_SCAN,
        FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::core::PCWSTR;

    let procs = get_ioring_procs().ok_or_else(|| "IoRing APIs unavailable".to_string())?;
    let write_fn = procs
        .build_write_file
        .ok_or_else(|| "BuildIoRingWriteFile unavailable".to_string())?;
    if windows_build_number() < IORING_MIN_WIN11_BUILD {
        return Err("Windows build below 22621".to_string());
    }
    if size_bytes == 0 {
        std::fs::write(dst, []).map_err(|e| format!("Failed creating empty destination: {e}"))?;
        return Ok(0);
    }

    let src_w: Vec<u16> = src
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let dst_w: Vec<u16> = dst
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let src_h = unsafe {
        CreateFileW(
            PCWSTR(src_w.as_ptr()),
            FILE_GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_SEQUENTIAL_SCAN,
            None,
        )
    }
    .map_err(|e| format!("Open source failed: {e}"))?;

    let dst_h = match unsafe {
        CreateFileW(
            PCWSTR(dst_w.as_ptr()),
            FILE_GENERIC_WRITE.0,
            FILE_SHARE_READ,
            None,
            CREATE_ALWAYS,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_SEQUENTIAL_SCAN,
            None,
        )
    } {
        Ok(h) => h,
        Err(e) => {
            unsafe {
                let _ = CloseHandle(src_h);
            }
            return Err(format!("Create destination failed: {e}"));
        }
    };

    let mut ring: *mut core::ffi::c_void = std::ptr::null_mut();
    let flags = IoringCreateFlags::default();
    let mut create_hr = -1i32;
    for ver in [3u32, 2u32] {
        create_hr = unsafe { (procs.create_io_ring)(ver, flags, 64, 64, &mut ring) };
        if create_hr >= 0 && !ring.is_null() {
            break;
        }
    }
    if create_hr < 0 || ring.is_null() {
        unsafe {
            let _ = CloseHandle(src_h);
            let _ = CloseHandle(dst_h);
        }
        return Err(format!("CreateIoRing v2/v3 failed: 0x{:08X}", create_hr as u32));
    }

    let mut slot_buffers: Vec<Vec<u8>> = (0..IORING_BATCH_SLOTS)
        .map(|_| vec![0u8; IORING_SLOT_BYTES])
        .collect();
    let src_ref = IoringHandleRef {
        kind: 0,
        handle: src_h.0 as isize,
    };
    let dst_ref = IoringHandleRef {
        kind: 0,
        handle: dst_h.0 as isize,
    };

    let mut offset = 0u64;
    let mut total_written = 0u64;
    let mut io_err: Option<String> = None;

    while offset < size_bytes {
        let mut batch_spans: Vec<(usize, u64, u32)> = Vec::with_capacity(IORING_BATCH_SLOTS);
        for (slot_idx, buf) in slot_buffers.iter_mut().enumerate() {
            if offset >= size_bytes {
                break;
            }
            let chunk = (size_bytes - offset).min(IORING_SLOT_BYTES as u64) as u32;
            let data_ref = IoringBufferRef {
                kind: 0,
                address: buf.as_mut_ptr().cast::<core::ffi::c_void>(),
            };
            let hr = unsafe {
                (procs.build_read_file)(ring, src_ref, data_ref, chunk, offset, slot_idx, 0)
            };
            if hr < 0 {
                io_err = Some(format!("BuildIoRingReadFile failed: 0x{:08X}", hr as u32));
                break;
            }
            batch_spans.push((slot_idx, offset, chunk));
            offset = offset.saturating_add(chunk as u64);
        }
        if io_err.is_some() || batch_spans.is_empty() {
            break;
        }
        let queued = batch_spans.len() as u32;
        let mut submitted = 0u32;
        let submit_hr = unsafe { (procs.submit_io_ring)(ring, queued, 15_000, &mut submitted) };
        if submit_hr < 0 {
            io_err = Some(format!("SubmitIoRing read failed: 0x{:08X}", submit_hr as u32));
            break;
        }
        let mut read_sizes = [0u32; IORING_BATCH_SLOTS];
        let mut drained = 0u32;
        while drained < queued {
            let mut cqe = IoringCqe::default();
            if unsafe { (procs.pop_completion)(ring, &mut cqe) } != 0 {
                break;
            }
            drained += 1;
            if cqe.result_code < 0 {
                io_err = Some(format!(
                    "IoRing read CQE failed: 0x{:08X}",
                    cqe.result_code as u32
                ));
                break;
            }
            if cqe.user_data < read_sizes.len() {
                read_sizes[cqe.user_data] = cqe.information as u32;
            }
        }
        if io_err.is_some() {
            break;
        }

        let mut write_queued = 0u32;
        for &(slot_idx, span_off, _req_len) in &batch_spans {
            let actual = read_sizes[slot_idx];
            if actual == 0 {
                continue;
            }
            let data_ref = IoringBufferRef {
                kind: 0,
                address: slot_buffers[slot_idx].as_mut_ptr().cast::<core::ffi::c_void>(),
            };
            let hr = unsafe {
                write_fn(ring, dst_ref, data_ref, actual, span_off, 0, slot_idx, 0)
            };
            if hr < 0 {
                io_err = Some(format!("BuildIoRingWriteFile failed: 0x{:08X}", hr as u32));
                break;
            }
            write_queued += 1;
        }
        if io_err.is_some() {
            break;
        }
        if write_queued > 0 {
            let mut w_submitted = 0u32;
            let w_hr = unsafe { (procs.submit_io_ring)(ring, write_queued, 15_000, &mut w_submitted) };
            if w_hr < 0 {
                io_err = Some(format!("SubmitIoRing write failed: 0x{:08X}", w_hr as u32));
                break;
            }
            let mut w_drained = 0u32;
            while w_drained < write_queued {
                let mut cqe = IoringCqe::default();
                if unsafe { (procs.pop_completion)(ring, &mut cqe) } != 0 {
                    break;
                }
                w_drained += 1;
                if cqe.result_code < 0 {
                    io_err = Some(format!(
                        "IoRing write CQE failed: 0x{:08X}",
                        cqe.result_code as u32
                    ));
                    break;
                }
                total_written = total_written.saturating_add(cqe.information as u64);
            }
            if io_err.is_some() {
                break;
            }
        }
    }

    unsafe {
        let _ = (procs.close_io_ring)(ring);
        let _ = CloseHandle(src_h);
        let _ = CloseHandle(dst_h);
    }

    if let Some(err) = io_err {
        return Err(err);
    }
    Ok(total_written)
}

/// Copies `src` to `dst` using Windows 11 Kernel `IoRing` (`ReadFile` + `WriteFile`) when available,
/// falling back cleanly to 64 KiB streaming buffered copy.
pub fn ioring_copy_file(src: &Path, dst: &Path) -> Result<IoRingTransferReport, String> {
    use std::io::{Read, Write};

    let started = Instant::now();
    let meta = fs::metadata(src)
        .map_err(|e| format!("Failed reading source metadata {}: {e}", src.display()))?;
    let size_bytes = meta.len();
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed creating destination parent {}: {e}", parent.display()))?;
    }

    #[cfg(target_os = "windows")]
    if size_bytes >= 64 * 1024
        && let Ok(written) = try_ioring_copy_file_windows(src, dst, size_bytes)
        && written == size_bytes
    {
        return Ok(IoRingTransferReport {
            bytes_transferred: written,
            used_ioring_api: true,
            elapsed_ms: started.elapsed().as_millis() as u64,
            backend: format!("Win11 Kernel IoRing (Build {})", windows_build_number()),
        });
    }

    let mut reader = fs::File::open(src)
        .map_err(|e| format!("Failed opening source {}: {e}", src.display()))?;
    let mut writer = fs::File::create(dst)
        .map_err(|e| format!("Failed creating destination {}: {e}", dst.display()))?;
    let mut buf = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("Stream read failed on {}: {e}", src.display()))?;
        if n == 0 {
            break;
        }
        writer
            .write_all(&buf[..n])
            .map_err(|e| format!("Stream write failed on {}: {e}", dst.display()))?;
        total = total.saturating_add(n as u64);
    }
    writer
        .flush()
        .map_err(|e| format!("Stream flush failed on {}: {e}", dst.display()))?;

    Ok(IoRingTransferReport {
        bytes_transferred: total,
        used_ioring_api: false,
        elapsed_ms: started.elapsed().as_millis() as u64,
        backend: "64 KiB Streaming Fallback".to_string(),
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

        let rvz_path = dir.join("metroid_prime.rvz");
        fs::write(&rvz_path, &payload_vpk).unwrap();
        let single = warm_single_file_with_options(&rvz_path, 64 * 1024, 25).unwrap();
        assert_eq!(single.files_warmed, 1);
        assert_eq!(single.warmed_bytes, 256 * 1024);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ioring_capability_probe_and_fallback_contract() {
        let cap = probe_ioring_support();
        assert_eq!(cap.min_required_build, IORING_MIN_WIN11_BUILD);
        assert!(!cap.backend.is_empty());

        let dir = std::env::temp_dir().join(format!("duke_ioring_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let pak = dir.join("chunk_test.pak");
        fs::write(&pak, vec![0xA5u8; 768 * 1024]).unwrap();

        let report = warm_single_file_with_options(&pak, 64 * 1024, 25).unwrap();
        assert_eq!(report.files_warmed, 1);
        assert_eq!(report.warmed_bytes, 768 * 1024);
        #[cfg(target_os = "windows")]
        assert_eq!(report.used_ioring_api, cap.supported);

        let dst = dir.join("chunk_copy.pak");
        let xfer = ioring_copy_file(&pak, &dst).unwrap();
        assert_eq!(xfer.bytes_transferred, 768 * 1024);
        assert_eq!(fs::read(&dst).unwrap(), fs::read(&pak).unwrap());

        let _ = fs::remove_dir_all(&dir);
    }
}
