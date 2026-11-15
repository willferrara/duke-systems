//! Single-Player Live Memory Trainer & `NtSetTimerResolution` Speedhack Overlay (`duke_sandbox.rs`).
//!
//! Provides anti-cheat-guarded single-player memory value scanning/editing and Windows NT timer
//! resolution scaling directly inside the Guide-button overlay (`#overlay-tab-sandbox`).
//! Automatically disables all process attachment whenever online anti-cheat drivers or processes
//! (`EasyAntiCheat`, `BattlEye`, `Vanguard`, `FACEIT`) are active on the system.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const MAX_SCAN_RESULTS: usize = 50_000;
const CHUNK_READ_BYTES: usize = 256 * 1024; // Heap-allocated 256 KiB read chunk

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AntiCheatGuardReport {
    pub blocked: bool,
    pub detected_systems: Vec<String>,
    pub matched_artifacts: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryMatchPreview {
    pub address_hex: String,
    pub region_protect: String,
    pub value_type: String,
    pub current_value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryScanResponse {
    pub pid: u32,
    pub value_type: String,
    pub total_matches: usize,
    pub truncated: bool,
    pub matches: Vec<MemoryMatchPreview>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxTrainerStatus {
    pub attached_pid: u32,
    pub anti_cheat_blocked: bool,
    pub anti_cheat_report: AntiCheatGuardReport,
    pub speedhack_scale: f64,
    pub timer_resolution_ms: f64,
    pub cached_match_count: usize,
    pub status_text: String,
}

#[derive(Debug, Clone)]
struct CachedScanSession {
    value_type: String,
    addresses: Vec<usize>,
}

fn scan_cache() -> &'static Mutex<HashMap<u32, CachedScanSession>> {
    static CACHE: OnceLock<Mutex<HashMap<u32, CachedScanSession>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn active_speed_state() -> &'static Mutex<(f64, f64)> {
    static SPEED: OnceLock<Mutex<(f64, f64)>> = OnceLock::new();
    SPEED.get_or_init(|| Mutex::new((1.0, 15.625)))
}

/// Evaluates process names and loaded kernel driver names against online anti-cheat signatures
/// (`EasyAntiCheat`, `BattlEye`, `Vanguard`, `FACEIT`).
pub fn evaluate_anticheat_guard(exe_names: &[&str], driver_names: &[&str]) -> AntiCheatGuardReport {
    const SIGNATURES: &[(&str, &str)] = &[
        ("easyanticheat", "EasyAntiCheat"),
        ("beservice", "BattlEye"),
        ("bedaisy", "BattlEye"),
        ("belauncher", "BattlEye"),
        ("vgc.exe", "Riot Vanguard"),
        ("vgtray", "Riot Vanguard"),
        ("vgk.sys", "Riot Vanguard"),
        ("vanguard", "Riot Vanguard"),
        ("faceit", "FACEIT Anti-Cheat"),
    ];

    let mut systems = Vec::new();
    let mut matched = Vec::new();

    for raw in exe_names.iter().chain(driver_names.iter()) {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        for (needle, label) in SIGNATURES {
            if lower.contains(needle) {
                if !systems.iter().any(|s| s == *label) {
                    systems.push((*label).to_string());
                }
                if !matched.iter().any(|m| m == trimmed) {
                    matched.push(trimmed.to_string());
                }
            }
        }
    }

    let blocked = !systems.is_empty();
    let summary = if blocked {
        format!(
            "Trainer & Speedhack disabled: detected {} ({}).",
            systems.join(", "),
            matched.join(", ")
        )
    } else {
        "Single-Player Guard Clear: zero online anti-cheat processes or kernel drivers running.".to_string()
    };

    AntiCheatGuardReport {
        blocked,
        detected_systems: systems,
        matched_artifacts: matched,
        summary,
    }
}

#[cfg(windows)]
pub fn scan_anticheat_drivers_and_processes(exe_path_hint: Option<&str>) -> AntiCheatGuardReport {
    use std::ffi::c_void;
    use windows::core::{PCSTR, PCWSTR};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
    #[repr(C)]
    #[allow(non_snake_case)]
    struct ProcessEntry32W {
        dwSize: u32,
        cntUsage: u32,
        th32ProcessID: u32,
        th32DefaultHeapID: usize,
        th32ModuleID: u32,
        cntThreads: u32,
        th32ParentProcessID: u32,
        pcPriClassBase: i32,
        dwFlags: u32,
        szExeFile: [u16; 260],
    }

    type CreateSnapshotFn = unsafe extern "system" fn(u32, u32) -> isize;
    type ProcessFirstWFn = unsafe extern "system" fn(isize, *mut ProcessEntry32W) -> i32;
    type ProcessNextWFn = unsafe extern "system" fn(isize, *mut ProcessEntry32W) -> i32;
    type EnumDeviceDriversFn = unsafe extern "system" fn(*mut *mut c_void, u32, *mut u32) -> i32;
    type GetDriverBaseNameWFn = unsafe extern "system" fn(*mut c_void, *mut u16, u32) -> u32;

    let mut procs = Vec::new();
    let mut drivers = Vec::new();

    unsafe {
        let k32_w: Vec<u16> = "kernel32.dll\0".encode_utf16().collect();
        if let Ok(k32) = LoadLibraryW(PCWSTR(k32_w.as_ptr())) {
            if !k32.is_invalid() {
                if let (Some(snap_p), Some(first_p), Some(next_p)) = (
                    GetProcAddress(k32, PCSTR(c"CreateToolhelp32Snapshot".as_ptr().cast())),
                    GetProcAddress(k32, PCSTR(c"Process32FirstW".as_ptr().cast())),
                    GetProcAddress(k32, PCSTR(c"Process32NextW".as_ptr().cast())),
                ) {
                    let create_snap: CreateSnapshotFn = std::mem::transmute(snap_p);
                    let proc_first: ProcessFirstWFn = std::mem::transmute(first_p);
                    let proc_next: ProcessNextWFn = std::mem::transmute(next_p);
                    let snap = create_snap(TH32CS_SNAPPROCESS, 0);
                    if snap != -1 && snap != 0 {
                        let mut pe: ProcessEntry32W = std::mem::zeroed();
                        pe.dwSize = std::mem::size_of::<ProcessEntry32W>() as u32;
                        if proc_first(snap, &mut pe) != 0 {
                            loop {
                                let end = pe
                                    .szExeFile
                                    .iter()
                                    .position(|&c| c == 0)
                                    .unwrap_or(pe.szExeFile.len());
                                let name = String::from_utf16_lossy(&pe.szExeFile[..end]);
                                if !name.is_empty() {
                                    procs.push(name);
                                }
                                if proc_next(snap, &mut pe) == 0 {
                                    break;
                                }
                            }
                        }
                        let _ = CloseHandle(windows::Win32::Foundation::HANDLE(snap as *mut c_void));
                    }
                }
            }
        }

        let psapi_w: Vec<u16> = "psapi.dll\0".encode_utf16().collect();
        if let Ok(psapi) = LoadLibraryW(PCWSTR(psapi_w.as_ptr())) {
            if !psapi.is_invalid() {
                if let (Some(enum_p), Some(name_p)) = (
                    GetProcAddress(psapi, PCSTR(c"EnumDeviceDrivers".as_ptr().cast())),
                    GetProcAddress(psapi, PCSTR(c"GetDeviceDriverBaseNameW".as_ptr().cast())),
                ) {
                    let enum_drv: EnumDeviceDriversFn = std::mem::transmute(enum_p);
                    let drv_name: GetDriverBaseNameWFn = std::mem::transmute(name_p);
                    // Heap buffer for up to 1024 loaded kernel driver image pointers
                    let mut ptrs: Vec<*mut c_void> = vec![std::ptr::null_mut(); 1024];
                    let mut needed = 0u32;
                    let cb = (ptrs.len() * std::mem::size_of::<*mut c_void>()) as u32;
                    if enum_drv(ptrs.as_mut_ptr(), cb, &mut needed) != 0 {
                        let count = ((needed as usize) / std::mem::size_of::<*mut c_void>())
                            .min(ptrs.len());
                        let mut name_buf = [0u16; 260];
                        for &ptr in &ptrs[..count] {
                            let len = drv_name(ptr, name_buf.as_mut_ptr(), name_buf.len() as u32);
                            if len > 0 {
                                drivers.push(String::from_utf16_lossy(&name_buf[..len as usize]));
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(hint) = exe_path_hint {
        if let Some(parent) = std::path::Path::new(hint).parent() {
            for folder in ["EasyAntiCheat", "BattlEye", "Vanguard", "FACEIT"] {
                if parent.join(folder).is_dir() {
                    procs.push(folder.to_string());
                }
            }
        }
    }

    let proc_refs: Vec<&str> = procs.iter().map(String::as_str).collect();
    let drv_refs: Vec<&str> = drivers.iter().map(String::as_str).collect();
    evaluate_anticheat_guard(&proc_refs, &drv_refs)
}

#[cfg(not(windows))]
pub fn scan_anticheat_drivers_and_processes(_exe_path_hint: Option<&str>) -> AntiCheatGuardReport {
    evaluate_anticheat_guard(&[], &[])
}

pub fn encode_value_bytes(value_type: &str, value_str: &str) -> Result<(Vec<u8>, usize), String> {
    let clean = value_str.trim();
    match value_type.trim().to_ascii_lowercase().as_str() {
        "i32" => {
            let v: i32 = clean
                .parse()
                .map_err(|_| format!("Invalid i32 integer: '{}'", clean))?;
            Ok((v.to_le_bytes().to_vec(), 4))
        }
        "u32" => {
            let v: u32 = clean
                .parse()
                .map_err(|_| format!("Invalid u32 integer: '{}'", clean))?;
            Ok((v.to_le_bytes().to_vec(), 4))
        }
        "i64" => {
            let v: i64 = clean
                .parse()
                .map_err(|_| format!("Invalid i64 integer: '{}'", clean))?;
            Ok((v.to_le_bytes().to_vec(), 8))
        }
        "f32" => {
            let v: f32 = clean
                .parse()
                .map_err(|_| format!("Invalid f32 float: '{}'", clean))?;
            Ok((v.to_le_bytes().to_vec(), 4))
        }
        "f64" => {
            let v: f64 = clean
                .parse()
                .map_err(|_| format!("Invalid f64 float: '{}'", clean))?;
            Ok((v.to_le_bytes().to_vec(), 8))
        }
        other => Err(format!("Unsupported trainer value type: '{}'", other)),
    }
}

pub fn format_value_bytes(value_type: &str, slice: &[u8]) -> String {
    match value_type.trim().to_ascii_lowercase().as_str() {
        "i32" if slice.len() >= 4 => {
            i32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]).to_string()
        }
        "u32" if slice.len() >= 4 => {
            u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]).to_string()
        }
        "i64" if slice.len() >= 8 => i64::from_le_bytes([
            slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
        ])
        .to_string(),
        "f32" if slice.len() >= 4 => {
            let v = f32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]);
            format!("{v:.3}")
        }
        "f64" if slice.len() >= 8 => {
            let v = f64::from_le_bytes([
                slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
            ]);
            format!("{v:.4}")
        }
        _ => "—".to_string(),
    }
}

/// Scans a contiguous heap slice for exact-value aligned offsets.
pub fn find_matching_offsets_in_buffer(
    buf: &[u8],
    needle: &[u8],
    align: usize,
) -> Vec<usize> {
    if needle.is_empty() || buf.len() < needle.len() {
        return Vec::new();
    }
    let step = align.max(1);
    let mut hits = Vec::new();
    let limit = buf.len() - needle.len();
    let mut off = 0usize;
    while off <= limit {
        if &buf[off..off + needle.len()] == needle {
            hits.push(off);
            if hits.len() >= 4096 {
                break;
            }
        }
        off += step;
    }
    hits
}

/// Maps requested speedhack multiplier (`0.25..=5.0`) to safe `NtSetTimerResolution`
/// 100-nanosecond timer period (`5000` = 0.5 ms high-res when active, `156250` = 15.625 ms default).
pub fn speed_scale_to_100ns_units(scale: f64) -> (f64, u32) {
    let clamped = scale.clamp(0.25, 5.0);
    if (clamped - 1.0).abs() < 0.02 {
        return (1.0, 156_250);
    }
    // Scale 100ns resolution within Win32 NT timer range cleanly
    let units = ((156_250.0 / clamped) as u32).clamp(5_000, 156_250);
    (clamped, units)
}

#[cfg(windows)]
fn apply_nt_timer_resolution(desired_100ns: u32, enable: bool) -> Result<f64, String> {
    use windows::core::{PCSTR, PCWSTR};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    type NtSetTimerResFn = unsafe extern "system" fn(u32, u8, *mut u32) -> i32;
    let ntdll_w: Vec<u16> = "ntdll.dll\0".encode_utf16().collect();
    let mut actual = desired_100ns;
    unsafe {
        let h = LoadLibraryW(PCWSTR(ntdll_w.as_ptr()))
            .map_err(|e| format!("Failed loading ntdll.dll: {e}"))?;
        let p = GetProcAddress(h, PCSTR(c"NtSetTimerResolution".as_ptr().cast()))
            .ok_or_else(|| "NtSetTimerResolution symbol missing.".to_string())?;
        let func: NtSetTimerResFn = std::mem::transmute(p);
        let status = func(desired_100ns, if enable { 1 } else { 0 }, &mut actual);
        if status < 0 {
            return Err(format!("NtSetTimerResolution NTSTATUS 0x{:08X}", status as u32));
        }
    }
    Ok((actual as f64) / 10_000.0)
}

#[cfg(not(windows))]
fn apply_nt_timer_resolution(desired_100ns: u32, _enable: bool) -> Result<f64, String> {
    Ok((desired_100ns as f64) / 10_000.0)
}

#[cfg(windows)]
fn scan_or_refine_process_memory(
    pid: u32,
    value_type: &str,
    needle: &[u8],
    align: usize,
    prior: Option<Vec<usize>>,
) -> Result<(Vec<usize>, Vec<MemoryMatchPreview>), String> {
    use std::ffi::c_void;
    use windows::core::{PCSTR, PCWSTR};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};

    const MEM_COMMIT: u32 = 0x1000;
    const PAGE_READWRITE: u32 = 0x04;
    const PAGE_EXECUTE_READWRITE: u32 = 0x40;
    const PAGE_GUARD: u32 = 0x100;

    #[repr(C)]
    #[allow(non_snake_case)]
    struct MemoryBasicInformation {
        BaseAddress: *const c_void,
        AllocationBase: *const c_void,
        AllocationProtect: u32,
        PartitionId: u16,
        RegionSize: usize,
        State: u32,
        Protect: u32,
        Type: u32,
    }

    type VirtualQueryExFn =
        unsafe extern "system" fn(*mut c_void, *const c_void, *mut MemoryBasicInformation, usize) -> usize;
    type ReadProcessMemoryFn =
        unsafe extern "system" fn(*mut c_void, *const c_void, *mut c_void, usize, *mut usize) -> i32;

    let k32_w: Vec<u16> = "kernel32.dll\0".encode_utf16().collect();
    let (vqx_fn, rpm_fn) = unsafe {
        let k32 = LoadLibraryW(PCWSTR(k32_w.as_ptr()))
            .map_err(|e| format!("Failed loading kernel32.dll: {e}"))?;
        let vqx_p = GetProcAddress(k32, PCSTR(c"VirtualQueryEx".as_ptr().cast()))
            .ok_or_else(|| "VirtualQueryEx missing".to_string())?;
        let rpm_p = GetProcAddress(k32, PCSTR(c"ReadProcessMemory".as_ptr().cast()))
            .ok_or_else(|| "ReadProcessMemory missing".to_string())?;
        (
            std::mem::transmute::<unsafe extern "system" fn() -> isize, VirtualQueryExFn>(vqx_p),
            std::mem::transmute::<unsafe extern "system" fn() -> isize, ReadProcessMemoryFn>(rpm_p),
        )
    };

    let proc_handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) }
        .map_err(|e| format!("Cannot attach to PID {pid} for read scan: {e}"))?;

    let raw_handle = proc_handle.0;
    let mut matches_addrs = Vec::new();
    let mut chunk = vec![0u8; CHUNK_READ_BYTES];

    if let Some(prev_addrs) = prior {
        let mut small = [0u8; 16];
        for addr in prev_addrs {
            let mut bytes_read = 0usize;
            let ok = unsafe {
                rpm_fn(
                    raw_handle,
                    addr as *const c_void,
                    small.as_mut_ptr() as *mut c_void,
                    needle.len(),
                    &mut bytes_read,
                )
            };
            if ok != 0 && bytes_read == needle.len() && &small[..needle.len()] == needle {
                matches_addrs.push(addr);
                if matches_addrs.len() >= MAX_SCAN_RESULTS {
                    break;
                }
            }
        }
    } else {
        let mut cur_addr: usize = 0x10_000;
        let max_addr: usize = if cfg!(target_pointer_width = "64") {
            0x7FFF_FFFF_0000
        } else {
            0x7FFF_0000
        };
        let mut mbi: MemoryBasicInformation = unsafe { std::mem::zeroed() };
        while cur_addr < max_addr && matches_addrs.len() < MAX_SCAN_RESULTS {
            let queried = unsafe {
                vqx_fn(
                    raw_handle,
                    cur_addr as *const c_void,
                    &mut mbi,
                    std::mem::size_of::<MemoryBasicInformation>(),
                )
            };
            if queried == 0 || mbi.RegionSize == 0 {
                break;
            }
            let base = mbi.BaseAddress as usize;
            let size = mbi.RegionSize;
            let writable = (mbi.Protect & PAGE_READWRITE != 0)
                || (mbi.Protect & PAGE_EXECUTE_READWRITE != 0);
            if mbi.State == MEM_COMMIT && writable && (mbi.Protect & PAGE_GUARD == 0) {
                let mut offset = 0usize;
                while offset < size && matches_addrs.len() < MAX_SCAN_RESULTS {
                    let to_read = (size - offset).min(CHUNK_READ_BYTES);
                    let mut read_len = 0usize;
                    let ok = unsafe {
                        rpm_fn(
                            raw_handle,
                            (base + offset) as *const c_void,
                            chunk.as_mut_ptr() as *mut c_void,
                            to_read,
                            &mut read_len,
                        )
                    };
                    if ok != 0 && read_len >= needle.len() {
                        for rel in find_matching_offsets_in_buffer(&chunk[..read_len], needle, align) {
                            matches_addrs.push(base + offset + rel);
                            if matches_addrs.len() >= MAX_SCAN_RESULTS {
                                break;
                            }
                        }
                    }
                    offset += to_read;
                }
            }
            cur_addr = match base.checked_add(size) {
                Some(next) if next > cur_addr => next,
                _ => break,
            };
        }
    }

    let previews: Vec<MemoryMatchPreview> = matches_addrs
        .iter()
        .take(25)
        .map(|&addr| MemoryMatchPreview {
            address_hex: format!("0x{:X}", addr),
            region_protect: "RW".to_string(),
            value_type: value_type.to_string(),
            current_value: format_value_bytes(value_type, needle),
        })
        .collect();

    unsafe {
        let _ = CloseHandle(proc_handle);
    }
    Ok((matches_addrs, previews))
}

#[cfg(not(windows))]
fn scan_or_refine_process_memory(
    _pid: u32,
    value_type: &str,
    _needle: &[u8],
    _align: usize,
    _prior: Option<Vec<usize>>,
) -> Result<(Vec<usize>, Vec<MemoryMatchPreview>), String> {
    let _ = value_type;
    Err("Live process memory trainer requires Windows.".to_string())
}

#[cfg(windows)]
fn write_process_memory_bytes(pid: u32, addr: usize, payload: &[u8]) -> Result<(), String> {
    use std::ffi::c_void;
    use windows::core::{PCSTR, PCWSTR};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
    };

    type WriteProcessMemoryFn =
        unsafe extern "system" fn(*mut c_void, *mut c_void, *const c_void, usize, *mut usize) -> i32;

    let k32_w: Vec<u16> = "kernel32.dll\0".encode_utf16().collect();
    let wpm_fn = unsafe {
        let k32 = LoadLibraryW(PCWSTR(k32_w.as_ptr()))
            .map_err(|e| format!("Failed loading kernel32.dll: {e}"))?;
        let p = GetProcAddress(k32, PCSTR(c"WriteProcessMemory".as_ptr().cast()))
            .ok_or_else(|| "WriteProcessMemory missing".to_string())?;
        std::mem::transmute::<unsafe extern "system" fn() -> isize, WriteProcessMemoryFn>(p)
    };

    let proc_handle = unsafe {
        OpenProcess(
            PROCESS_VM_OPERATION | PROCESS_VM_READ | PROCESS_VM_WRITE,
            false,
            pid,
        )
    }
    .map_err(|e| format!("Cannot open PID {pid} for write: {e}"))?;

    let mut written = 0usize;
    let ok = unsafe {
        wpm_fn(
            proc_handle.0,
            addr as *mut c_void,
            payload.as_ptr() as *const c_void,
            payload.len(),
            &mut written,
        )
    };
    unsafe {
        let _ = CloseHandle(proc_handle);
    }
    if ok == 0 || written != payload.len() {
        return Err(format!(
            "WriteProcessMemory at 0x{addr:X} failed (wrote {written}/{} bytes).",
            payload.len()
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn write_process_memory_bytes(_pid: u32, _addr: usize, _payload: &[u8]) -> Result<(), String> {
    Err("Memory editing requires Windows.".to_string())
}

#[tauri::command]
pub async fn get_sandbox_trainer_status(
    pid: Option<u32>,
    exe_path: Option<String>,
) -> Result<SandboxTrainerStatus, String> {
    let target_pid = pid.unwrap_or(0);
    let report = tokio::task::spawn_blocking(move || {
        scan_anticheat_drivers_and_processes(exe_path.as_deref())
    })
    .await
    .map_err(|e| format!("Anti-cheat probe failed: {e}"))?;

    // If an online anti-cheat is detected, automatically drop any cached handles & reset speedhack.
    if report.blocked {
        if let Ok(mut cache) = scan_cache().lock() {
            cache.clear();
        }
        if let Ok(mut sp) = active_speed_state().lock() {
            if (sp.0 - 1.0).abs() > 0.01 {
                let _ = apply_nt_timer_resolution(156_250, false);
                *sp = (1.0, 15.625);
            }
        }
    }

    let (scale, res_ms) = active_speed_state()
        .lock()
        .map(|g| *g)
        .unwrap_or((1.0, 15.625));
    let cached_count = scan_cache()
        .lock()
        .ok()
        .and_then(|c| c.get(&target_pid).map(|s| s.addresses.len()))
        .unwrap_or(0);

    let status_text = if report.blocked {
        report.summary.clone()
    } else if target_pid == 0 {
        "Single-Player Guard Clear · Launch an offline game to attach memory scanner.".to_string()
    } else {
        format!(
            "Attached to single-player PID {} · {:.2}x timer ({:.2} ms)",
            target_pid, scale, res_ms
        )
    };

    Ok(SandboxTrainerStatus {
        attached_pid: if report.blocked { 0 } else { target_pid },
        anti_cheat_blocked: report.blocked,
        anti_cheat_report: report,
        speedhack_scale: scale,
        timer_resolution_ms: (res_ms * 100.0).round() / 100.0,
        cached_match_count: cached_count,
        status_text,
    })
}

#[tauri::command]
pub async fn sandbox_scan_memory(
    pid: u32,
    value_type: String,
    value_str: String,
    reset_scan: bool,
) -> Result<MemoryScanResponse, String> {
    if pid == 0 {
        return Err("No active single-player game PID to scan.".to_string());
    }
    let guard = tokio::task::spawn_blocking(|| scan_anticheat_drivers_and_processes(None))
        .await
        .map_err(|e| format!("Anti-cheat check failed: {e}"))?;
    if guard.blocked {
        return Err(guard.summary);
    }

    let (needle, align) = encode_value_bytes(&value_type, &value_str)?;
    let prior = if reset_scan {
        None
    } else {
        scan_cache()
            .lock()
            .ok()
            .and_then(|c| c.get(&pid).filter(|s| s.value_type == value_type).map(|s| s.addresses.clone()))
    };
    let is_refine = prior.is_some();
    let vtype = value_type.clone();

    let (all_addrs, previews) = tokio::task::spawn_blocking(move || {
        scan_or_refine_process_memory(pid, &vtype, &needle, align, prior)
    })
    .await
    .map_err(|e| format!("Memory scan worker failed: {e}"))??;

    let total = all_addrs.len();
    if let Ok(mut cache) = scan_cache().lock() {
        cache.insert(
            pid,
            CachedScanSession {
                value_type: value_type.clone(),
                addresses: all_addrs,
            },
        );
    }

    Ok(MemoryScanResponse {
        pid,
        value_type,
        total_matches: total,
        truncated: total >= MAX_SCAN_RESULTS,
        matches: previews,
        summary: format!(
            "{} found {} writable address(es) for {}.",
            if is_refine { "Refined scan" } else { "Initial scan" },
            total,
            value_str.trim()
        ),
    })
}

#[tauri::command]
pub async fn sandbox_write_memory_value(
    pid: u32,
    address_hex: String,
    value_type: String,
    new_value: String,
) -> Result<String, String> {
    if pid == 0 {
        return Err("No active game PID selected.".to_string());
    }
    let guard = tokio::task::spawn_blocking(|| scan_anticheat_drivers_and_processes(None))
        .await
        .map_err(|e| format!("Anti-cheat guard failed: {e}"))?;
    if guard.blocked {
        return Err(guard.summary);
    }
    let cleaned_hex = address_hex.trim().trim_start_matches("0x").trim_start_matches("0X");
    let addr = usize::from_str_radix(cleaned_hex, 16)
        .map_err(|_| format!("Invalid hexadecimal memory address: '{}'", address_hex))?;
    let (payload, _) = encode_value_bytes(&value_type, &new_value)?;

    tokio::task::spawn_blocking(move || write_process_memory_bytes(pid, addr, &payload))
        .await
        .map_err(|e| format!("Memory write worker failed: {e}"))??;

    Ok(format!(
        "Wrote {} ({}) to 0x{:X} in PID {}.",
        new_value.trim(),
        value_type,
        addr,
        pid
    ))
}

#[tauri::command]
pub async fn sandbox_set_speedhack_scale(
    pid: Option<u32>,
    scale: f64,
) -> Result<SandboxTrainerStatus, String> {
    let target_pid = pid.unwrap_or(0);
    let guard = tokio::task::spawn_blocking(|| scan_anticheat_drivers_and_processes(None))
        .await
        .map_err(|e| format!("Anti-cheat guard failed: {e}"))?;

    let (clamped_scale, units_100ns) = if guard.blocked {
        (1.0, 156_250)
    } else {
        speed_scale_to_100ns_units(scale)
    };
    let enable_custom = !guard.blocked && (clamped_scale - 1.0).abs() >= 0.02;
    let res_ms = apply_nt_timer_resolution(units_100ns, enable_custom)?;

    if let Ok(mut sp) = active_speed_state().lock() {
        *sp = (clamped_scale, res_ms);
    }
    if guard.blocked && (scale - 1.0).abs() >= 0.02 {
        return Err(guard.summary);
    }

    Ok(SandboxTrainerStatus {
        attached_pid: if guard.blocked { 0 } else { target_pid },
        anti_cheat_blocked: guard.blocked,
        anti_cheat_report: guard,
        speedhack_scale: clamped_scale,
        timer_resolution_ms: (res_ms * 100.0).round() / 100.0,
        cached_match_count: 0,
        status_text: if enable_custom {
            format!(
                "NtSetTimerResolution active: {:.2}x pacing ({:.2} ms tick).",
                clamped_scale, res_ms
            )
        } else {
            "Timer resolution restored to 1.00x standard Windows cadence.".to_string()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anticheat_guard_blocks_eac_battleye_vanguard_and_faceit() {
        let eac = evaluate_anticheat_guard(&["EasyAntiCheat_EOS.exe"], &[]);
        assert!(eac.blocked);
        assert!(eac.detected_systems.contains(&"EasyAntiCheat".to_string()));

        let be = evaluate_anticheat_guard(&[], &["BEDaisy.sys"]);
        assert!(be.blocked);
        assert!(be.detected_systems.contains(&"BattlEye".to_string()));

        let vg = evaluate_anticheat_guard(&["vgtray.exe"], &["vgk.sys"]);
        assert!(vg.blocked);
        assert!(vg.detected_systems.contains(&"Riot Vanguard".to_string()));

        let fi = evaluate_anticheat_guard(&["faceitservice.exe"], &[]);
        assert!(fi.blocked);
        assert!(fi.detected_systems.contains(&"FACEIT Anti-Cheat".to_string()));

        let clean = evaluate_anticheat_guard(&["eldenring.exe", "explorer.exe"], &["nvlddmkm.sys"]);
        assert!(!clean.blocked);
    }

    #[test]
    fn memory_pattern_scan_and_refine_roundtrip() {
        let mut buf = vec![0u8; 256];
        buf[16..20].copy_from_slice(&999i32.to_le_bytes());
        buf[64..68].copy_from_slice(&999i32.to_le_bytes());
        let (needle, align) = encode_value_bytes("i32", "999").expect("encode i32");
        let hits = find_matching_offsets_in_buffer(&buf, &needle, align);
        assert_eq!(hits, vec![16, 64]);
        assert_eq!(format_value_bytes("i32", &buf[16..20]), "999");

        let (f_needle, f_align) = encode_value_bytes("f32", "42.5").expect("encode f32");
        buf[128..132].copy_from_slice(&f_needle);
        assert_eq!(find_matching_offsets_in_buffer(&buf, &f_needle, f_align), vec![128]);
    }

    #[test]
    fn speedhack_timer_resolution_math_clamps_safely() {
        assert_eq!(speed_scale_to_100ns_units(1.0), (1.0, 156_250));
        let (s2, u2) = speed_scale_to_100ns_units(2.0);
        assert_eq!(s2, 2.0);
        assert!((5_000..=156_250).contains(&u2));
        let (s_max, u_max) = speed_scale_to_100ns_units(99.0);
        assert_eq!(s_max, 5.0);
        assert!((5_000..=156_250).contains(&u_max));
    }
}
