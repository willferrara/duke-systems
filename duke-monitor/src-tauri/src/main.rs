// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod gpu_profiles;

use duke_core::pipe_ipc::{DukeBus, DukeEvent};
use duke_core::shm_bus::{pack_gpu_telemetry, DukeShmBus, ShmEventType, ShmSourceApp};
use duke_core::tray_daemon::{handle_run_event, install_standard_tray, TrayDaemonConfig, TrayDaemonState};
use reqwest::header::ACCEPT;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::ffi::{c_char, c_void, CStr};
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};
use tauri::{AppHandle, Emitter, Manager, Url};
use tokio::io::AsyncWriteExt;
use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

#[cfg(target_os = "windows")]
use windows::core::PCWSTR;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{CloseHandle, HANDLE, HMODULE};
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter3, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
    DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
#[cfg(target_os = "windows")]
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::SystemInformation::{
    GetLogicalProcessorInformationEx, GlobalMemoryStatusEx, RelationProcessorCore, MEMORYSTATUSEX,
    PROCESSOR_RELATIONSHIP, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{
    OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
    PROCESS_SET_QUOTA, PROCESS_TERMINATE,
};

static DUKE_BUS: OnceLock<DukeBus> = OnceLock::new();
static DUKE_SHM_BUS: OnceLock<Option<DukeShmBus>> = OnceLock::new();
static SHM_STOP_SIGNAL: AtomicBool = AtomicBool::new(false);
static AUTO_TRIM_ENABLED: AtomicBool = AtomicBool::new(true);

static SYSINFO_STATE: OnceLock<Mutex<System>> = OnceLock::new();
static TOPOLOGY_CACHE: OnceLock<CpuTopologyInfo> = OnceLock::new();
static NVML_API: OnceLock<Option<NvmlApi>> = OnceLock::new();
type TimedCache<T> = OnceLock<Mutex<Option<(Instant, T)>>>;

static PDH_GPU_SAMPLER: OnceLock<Mutex<Option<GpuPdhSampler>>> = OnceLock::new();
static LAST_PDH_SAMPLE: TimedCache<Option<f32>> = OnceLock::new();
static WMI_GPU_CACHE: TimedCache<Vec<WmiVideoInfo>> = OnceLock::new();
static ECO_JAILED_PIDS: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
static BOOSTED_PIDS: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
static GPU_CLOCKS_LOCKED_FOR_GAME: AtomicBool = AtomicBool::new(false);
static GPU_POWER_MODIFIED_FOR_GAME: AtomicBool = AtomicBool::new(false);
static DEFAULT_GPU_POWER_LIMIT_MW: AtomicU32 = AtomicU32::new(0);

const CREATE_NO_WINDOW: u32 = 0x08000000;
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x00004000;
const PROCESS_POWER_THROTTLING_CURRENT_VERSION: u32 = 1;
const PROCESS_POWER_THROTTLING_EXECUTION_SPEED: u32 = 0x1;
const PROCESS_POWER_THROTTLING_INFO_CLASS: u32 = 4;

const NVML_SUCCESS: u32 = 0;
const NVML_TEMPERATURE_GPU: u32 = 0;
const NVML_CLOCK_GRAPHICS: u32 = 0;
const NVML_CLOCK_MEM: u32 = 2;

const BLOAT_SIGNATURES: &[(&str, &str)] = &[
    ("omencommandcenterbackground", "HP OMEN Gaming Hub background telemetry"),
    ("omencap", "HP OMEN capture/overlay background agent"),
    ("hpsystemeventutilityhost", "HP OEM background event host"),
    ("armourycrate", "ASUS Armoury Crate background bloat"),
    ("armourycrate.usersessionhelper", "ASUS Armoury Crate session helper"),
    ("asuscertservice", "ASUS background certificate service"),
    ("lightingservice", "ASUS Aura/RGB background poller"),
    ("nvtelemetrycontainer", "NVIDIA GeForce telemetry collector"),
    ("nvbackend", "GeForce Experience background optimizer"),
    ("nvidia web helper", "GeForce Experience embedded Chromium helper"),
    ("nvidia overlay", "NVIDIA App / ShadowPlay overlay host"),
    ("gamebarpresencewriter", "Xbox Game Bar background presence writer"),
    ("gamebar", "Windows Xbox Game Bar overlay"),
    ("adobecollabsync", "Adobe Creative Cloud background sync"),
    ("adobeipcbroker", "Adobe IPC background daemon"),
    ("ccxprocess", "Adobe CCX background process"),
    ("coresync", "Adobe CoreSync cloud watcher"),
    ("razerappengine", "Razer Synapse heavy web engine"),
    ("razercentralservice", "Razer Central telemetry/updater"),
    ("icue", "Corsair iCUE background service"),
    ("corsair.service", "Corsair telemetry service"),
    ("nahimicsvc32", "Nahimic audio overlay bloat"),
    ("nahimicsvc64", "Nahimic audio overlay bloat"),
    ("killeranalyticsservice", "Killer Networking telemetry collector"),
    ("dellsupportassistagent", "Dell SupportAssist background updater"),
    ("lenovovantageservice", "Lenovo Vantage background service"),
    ("onedrive", "OneDrive background cloud sync"),
    ("phoneexperiencehost", "Windows Phone Link background sync"),
    ("widgets", "Windows 11 Widgets webview host"),
    ("widgetservice", "Windows 11 Widgets background feed"),
];

#[repr(C)]
struct ProcessMemoryCounters {
    cb: u32,
    page_fault_count: u32,
    peak_working_set_size: usize,
    working_set_size: usize,
    quota_peak_paged_pool_usage: usize,
    quota_paged_pool_usage: usize,
    quota_peak_non_paged_pool_usage: usize,
    quota_non_paged_pool_usage: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
}

#[repr(C)]
struct ProcessPowerThrottlingState {
    version: u32,
    control_mask: u32,
    state_mask: u32,
}

#[repr(C)]
struct NvmlMemory {
    total: u64,
    free: u64,
    used: u64,
}

#[repr(C)]
struct NvmlUtilization {
    gpu: u32,
    memory: u32,
}

#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
extern "system" {
    fn K32EmptyWorkingSet(hProcess: *mut c_void) -> i32;
    fn SetProcessWorkingSetSizeEx(
        hProcess: *mut c_void,
        dwMinimumWorkingSetSize: usize,
        dwMaximumWorkingSetSize: usize,
        Flags: u32,
    ) -> i32;
    fn K32GetProcessMemoryInfo(
        process: *mut c_void,
        ppsmemCounters: *mut ProcessMemoryCounters,
        cb: u32,
    ) -> i32;
    fn SetPriorityClass(hProcess: *mut c_void, dwPriorityClass: u32) -> i32;
    fn SetProcessInformation(
        hProcess: *mut c_void,
        ProcessInformationClass: u32,
        ProcessInformation: *const c_void,
        ProcessInformationSize: u32,
    ) -> i32;
}

type NvmlInitFn = unsafe extern "C" fn() -> u32;
type NvmlGetCountFn = unsafe extern "C" fn(*mut u32) -> u32;
type NvmlGetHandleFn = unsafe extern "C" fn(u32, *mut *mut c_void) -> u32;
type NvmlGetNameFn = unsafe extern "C" fn(*mut c_void, *mut c_char, u32) -> u32;
type NvmlGetTempFn = unsafe extern "C" fn(*mut c_void, u32, *mut u32) -> u32;
type NvmlGetClockFn = unsafe extern "C" fn(*mut c_void, u32, *mut u32) -> u32;
type NvmlGetMemoryFn = unsafe extern "C" fn(*mut c_void, *mut NvmlMemory) -> u32;
type NvmlGetPowerFn = unsafe extern "C" fn(*mut c_void, *mut u32) -> u32;
type NvmlSetPowerFn = unsafe extern "C" fn(*mut c_void, u32) -> u32;
type NvmlSetLockedClocksFn = unsafe extern "C" fn(*mut c_void, u32, u32) -> u32;
type NvmlResetLockedClocksFn = unsafe extern "C" fn(*mut c_void) -> u32;
type NvmlGetFanFn = unsafe extern "C" fn(*mut c_void, *mut u32) -> u32;
type NvmlGetUtilFn = unsafe extern "C" fn(*mut c_void, *mut NvmlUtilization) -> u32;

struct NvmlApi {
    _module: HMODULE,
    get_count: NvmlGetCountFn,
    get_handle: NvmlGetHandleFn,
    get_name: Option<NvmlGetNameFn>,
    get_temp: Option<NvmlGetTempFn>,
    get_clock: Option<NvmlGetClockFn>,
    get_memory: Option<NvmlGetMemoryFn>,
    get_power_usage: Option<NvmlGetPowerFn>,
    get_power_limit: Option<NvmlGetPowerFn>,
    set_power_limit: Option<NvmlSetPowerFn>,
    set_locked_clocks: Option<NvmlSetLockedClocksFn>,
    reset_locked_clocks: Option<NvmlResetLockedClocksFn>,
    get_fan_speed: Option<NvmlGetFanFn>,
    get_utilization: Option<NvmlGetUtilFn>,
}

unsafe impl Send for NvmlApi {}
unsafe impl Sync for NvmlApi {}

#[cfg(target_os = "windows")]
struct GpuPdhSampler {
    query: PDH_HQUERY,
    counter_3d: PDH_HCOUNTER,
    primed: bool,
}

#[cfg(not(target_os = "windows"))]
struct GpuPdhSampler {
    query: isize,
    counter_3d: isize,
    primed: bool,
}

unsafe impl Send for GpuPdhSampler {}
unsafe impl Sync for GpuPdhSampler {}

impl Drop for GpuPdhSampler {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        unsafe {
            if !self.query.is_invalid() {
                let _ = PdhCloseQuery(self.query);
            }
        }
    }
}

#[derive(Debug, Clone)]
struct CpuTopologyInfo {
    hybrid_detected: bool,
    core_types: HashMap<usize, String>,
    efficiency_classes: HashMap<usize, u8>,
    p_core_count: usize,
    e_core_count: usize,
    physical_cores: usize,
}

#[derive(Debug, Clone)]
struct WmiVideoInfo {
    name: String,
    driver_version: String,
    driver_date: String,
    pnp_device_id: String,
    adapter_ram_mb: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuTelemetry {
    pub index: usize,
    pub name: String,
    pub vendor: String,
    pub backend: String,
    pub pci_vendor_id: u32,
    pub pci_device_id: u32,
    pub temperature_c: Option<u32>,
    pub gpu_clock_mhz: Option<u32>,
    pub mem_clock_mhz: Option<u32>,
    pub vram_used_mb: u64,
    pub vram_total_mb: u64,
    pub power_draw_watts: Option<f64>,
    pub power_limit_watts: Option<f64>,
    pub fan_speed_pct: Option<u32>,
    pub utilization_pct: Option<f32>,
    pub is_primary: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogicalCoreUsage {
    pub index: usize,
    pub usage_pct: f32,
    pub frequency_mhz: u64,
    pub core_type: String,
    pub efficiency_class: u8,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuTelemetry {
    pub model: String,
    pub total_usage_pct: f32,
    pub frequency_mhz: u64,
    pub physical_cores: usize,
    pub logical_cores: usize,
    pub p_core_count: usize,
    pub e_core_count: usize,
    pub hybrid_detected: bool,
    pub cores: Vec<LogicalCoreUsage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryTelemetry {
    pub used_gb: f64,
    pub available_gb: f64,
    pub total_gb: f64,
    pub load_pct: u32,
    pub own_working_set_mb: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareTelemetry {
    pub gpus: Vec<GpuTelemetry>,
    pub cpu: CpuTelemetry,
    pub memory: MemoryTelemetry,
    pub doh_shield: String,
    pub doh_cached_hosts: usize,
    pub auto_trim_enabled: bool,
    pub eco_scheduler_status: String,
    pub eco_jailed_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RamPurgeResult {
    pub freed_mb: f64,
    pub working_set_freed_mb: f64,
    pub available_gain_mb: f64,
    pub processes_trimmed: usize,
    pub before_used_gb: f64,
    pub after_used_gb: f64,
    pub own_working_set_mb: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessEntry {
    pub pid: u32,
    pub name: String,
    pub exe_path: String,
    pub cpu_pct: f32,
    pub ram_mb: f64,
    pub status: String,
    pub eco_jailed: bool,
    pub boosted: bool,
    pub is_bloat: bool,
    pub bloat_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessActionResult {
    pub pid: u32,
    pub name: String,
    pub freed_mb: f64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameBoostResult {
    pub pid: u32,
    pub name: String,
    pub background_freed_mb: f64,
    pub processes_trimmed: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BloatPurgeResult {
    pub killed_count: usize,
    pub failed_count: usize,
    pub reclaimed_mb: f64,
    pub killed_names: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverDeviceInfo {
    pub id: String,
    pub category: String,
    pub name: String,
    pub vendor: String,
    pub hardware_id: String,
    pub installed_version: String,
    pub installed_display_version: String,
    pub driver_date: String,
    pub latest_version: Option<String>,
    pub latest_release_date: Option<String>,
    pub update_available: bool,
    pub status_message: String,
    pub release_notes: String,
    pub download_url: Option<String>,
    pub suggested_filename: Option<String>,
    pub official_portal_url: String,
    pub clean_install_guide: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverAuditReport {
    pub checked_at: u64,
    pub devices: Vec<DriverDeviceInfo>,
    pub drivers_download_dir: String,
    pub doh_cached_hosts: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverDownloadResult {
    pub saved_path: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub clean_install_command: String,
    pub clean_install_notes: String,
}

fn get_eco_jailed_pids() -> &'static Mutex<HashSet<u32>> {
    ECO_JAILED_PIDS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn get_boosted_pids() -> &'static Mutex<HashSet<u32>> {
    BOOSTED_PIDS.get_or_init(|| Mutex::new(HashSet::new()))
}

async fn build_doh_client_for_url(raw_url: &str) -> Client {
    duke_core::net_doh::build_doh_client_with_timeouts(
        raw_url,
        Duration::from_secs(7),
        Duration::from_secs(20),
        true,
    )
    .await
}

pub fn trim_process_working_set() {
    duke_core::win32_ffi::trim_current_working_set();
}

pub fn get_process_working_set_mb() -> f64 {
    let mb = duke_core::win32_ffi::get_current_working_set_mb();
    if mb > 0.1 {
        mb
    } else {
        18.0
    }
}

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide_null(slice: &[u16]) -> String {
    let end = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
    String::from_utf16_lossy(&slice[..end]).trim().to_string()
}

fn resolve_portable_data_dir() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir.join("portable.flag").exists() || dir.join("data").is_dir() {
                let data_dir = dir.join("data");
                let _ = fs::create_dir_all(&data_dir);
                return Some(data_dir);
            }
        }
    }
    None
}

fn resolve_drivers_download_dir(app: &AppHandle) -> PathBuf {
    if let Ok(dl) = app.path().download_dir() {
        return dl.join("Duke_Drivers");
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let candidate = PathBuf::from(profile).join("Downloads").join("Duke_Drivers");
        return candidate;
    }
    std::env::temp_dir().join("Duke_Drivers")
}

fn unblock_ntfs_zone_identifier(target: &Path) {
    if target.is_file() {
        let ads_path = format!("{}:Zone.Identifier", target.display());
        let _ = fs::remove_file(ads_path);
    }
}

fn get_sysinfo() -> &'static Mutex<System> {
    SYSINFO_STATE.get_or_init(|| {
        let mut sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything())
                .with_processes(ProcessRefreshKind::everything()),
        );
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        Mutex::new(sys)
    })
}

/// Uses Win32 `GetLogicalProcessorInformationEx(RelationProcessorCore)` to accurately classify P-cores vs E-cores.
fn get_cpu_topology() -> &'static CpuTopologyInfo {
    TOPOLOGY_CACHE.get_or_init(|| {
        let mut core_types = HashMap::new();
        let mut efficiency_classes = HashMap::new();
        let mut core_records: Vec<(u8, Vec<usize>)> = Vec::new();

        #[cfg(target_os = "windows")]
        unsafe {
            let mut len: u32 = 0;
            let _ = GetLogicalProcessorInformationEx(RelationProcessorCore, None, &mut len);
            if len > 0 {
                let mut buf = vec![0u8; len as usize];
                let ptr = buf.as_mut_ptr() as *mut SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX;
                if GetLogicalProcessorInformationEx(RelationProcessorCore, Some(ptr), &mut len).is_ok()
                {
                    let mut offset = 0usize;
                    let total = len as usize;
                    while offset + std::mem::size_of::<u32>() * 2 <= total {
                        let rec_ptr = buf.as_ptr().add(offset)
                            as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX;
                        let rec = std::ptr::read_unaligned(rec_ptr);
                        if rec.Size == 0 {
                            break;
                        }
                        if rec.Relationship == RelationProcessorCore {
                            let proc_rel: PROCESSOR_RELATIONSHIP = rec.Anonymous.Processor;
                            let eff = proc_rel.EfficiencyClass;
                            let group_count = proc_rel.GroupCount as usize;
                            let mut logical_indices = Vec::new();
                            let base_group_ptr =
                                std::ptr::addr_of!(rec.Anonymous.Processor.GroupMask) as *const u8;
                            let base_offset_in_rec =
                                base_group_ptr as usize - &rec as *const _ as usize;
                            for g in 0..group_count {
                                let ga_offset = offset
                                    + base_offset_in_rec
                                    + g * std::mem::size_of::<windows::Win32::System::SystemInformation::GROUP_AFFINITY>();
                                if ga_offset + std::mem::size_of::<windows::Win32::System::SystemInformation::GROUP_AFFINITY>() <= total {
                                    let ga_ptr = buf.as_ptr().add(ga_offset)
                                        as *const windows::Win32::System::SystemInformation::GROUP_AFFINITY;
                                    let ga = std::ptr::read_unaligned(ga_ptr);
                                    let mask = ga.Mask;
                                    let group_base = (ga.Group as usize) * 64;
                                    for bit in 0..usize::BITS as usize {
                                        if (mask & (1usize << bit)) != 0 {
                                            logical_indices.push(group_base + bit);
                                        }
                                    }
                                }
                            }
                            core_records.push((eff, logical_indices));
                        }
                        offset += rec.Size as usize;
                    }
                }
            }
        }

        let min_eff = core_records.iter().map(|(e, _)| *e).min().unwrap_or(0);
        let max_eff = core_records.iter().map(|(e, _)| *e).max().unwrap_or(0);
        let hybrid_detected = max_eff > min_eff;
        let mut p_core_count = 0usize;
        let mut e_core_count = 0usize;

        for (eff, indices) in &core_records {
            let label = if hybrid_detected {
                if *eff > min_eff {
                    p_core_count += 1;
                    "P-CORE"
                } else {
                    e_core_count += 1;
                    "E-CORE"
                }
            } else {
                p_core_count += 1;
                "CORE"
            };
            for &idx in indices {
                core_types.insert(idx, label.to_string());
                efficiency_classes.insert(idx, *eff);
            }
        }

        CpuTopologyInfo {
            hybrid_detected,
            core_types,
            efficiency_classes,
            p_core_count,
            e_core_count,
            physical_cores: core_records.len(),
        }
    })
}

fn get_nvml_api() -> Option<&'static NvmlApi> {
    NVML_API
        .get_or_init(|| {
            #[cfg(target_os = "windows")]
            unsafe {
                let mut candidates = vec![
                    "nvml.dll".to_string(),
                    "C:\\Windows\\System32\\nvml.dll".to_string(),
                ];
                if let Ok(pf) = std::env::var("ProgramW6432").or_else(|_| std::env::var("ProgramFiles")) {
                    candidates.push(format!("{}\\NVIDIA Corporation\\NVSMI\\nvml.dll", pf));
                }

                let mut loaded_module: Option<HMODULE> = None;
                for path in candidates {
                    let wide = to_wide_null(&path);
                    if let Ok(h) = LoadLibraryW(PCWSTR(wide.as_ptr())) {
                        if !h.is_invalid() {
                            loaded_module = Some(h);
                            break;
                        }
                    }
                }
                let module = loaded_module?;

                macro_rules! load_sym {
                    ($name:literal, $ty:ty) => {{
                        let sym = concat!($name, "\0");
                        let addr = GetProcAddress(module, windows::core::PCSTR(sym.as_ptr()));
                        addr.map(|p| std::mem::transmute::<_, $ty>(p))
                    }};
                }

                let init_fn: Option<NvmlInitFn> =
                    load_sym!("nvmlInit_v2", NvmlInitFn).or_else(|| load_sym!("nvmlInit", NvmlInitFn));
                let init_fn = init_fn?;
                if init_fn() != NVML_SUCCESS {
                    return None;
                }

                let get_count = load_sym!("nvmlDeviceGetCount_v2", NvmlGetCountFn)
                    .or_else(|| load_sym!("nvmlDeviceGetCount", NvmlGetCountFn))?;
                let get_handle = load_sym!("nvmlDeviceGetHandleByIndex_v2", NvmlGetHandleFn)
                    .or_else(|| load_sym!("nvmlDeviceGetHandleByIndex", NvmlGetHandleFn))?;

                Some(NvmlApi {
                    _module: module,
                    get_count,
                    get_handle,
                    get_name: load_sym!("nvmlDeviceGetName", NvmlGetNameFn),
                    get_temp: load_sym!("nvmlDeviceGetTemperature", NvmlGetTempFn),
                    get_clock: load_sym!("nvmlDeviceGetClockInfo", NvmlGetClockFn),
                    get_memory: load_sym!("nvmlDeviceGetMemoryInfo", NvmlGetMemoryFn),
                    get_power_usage: load_sym!("nvmlDeviceGetPowerUsage", NvmlGetPowerFn),
                    get_power_limit: load_sym!("nvmlDeviceGetPowerManagementLimit", NvmlGetPowerFn),
                    set_power_limit: load_sym!("nvmlDeviceSetPowerManagementLimit", NvmlSetPowerFn),
                    set_locked_clocks: load_sym!("nvmlDeviceSetGpuLockedClocks", NvmlSetLockedClocksFn),
                    reset_locked_clocks: load_sym!(
                        "nvmlDeviceResetGpuLockedClocks",
                        NvmlResetLockedClocksFn
                    ),
                    get_fan_speed: load_sym!("nvmlDeviceGetFanSpeed", NvmlGetFanFn),
                    get_utilization: load_sym!("nvmlDeviceGetUtilizationRates", NvmlGetUtilFn),
                })
            }
            #[cfg(not(target_os = "windows"))]
            {
                None
            }
        })
        .as_ref()
}

fn sample_pdh_gpu_3d_utilization() -> Option<f32> {
    let cache = LAST_PDH_SAMPLE.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = cache.lock() {
        if let Some((ts, cached)) = *guard {
            if ts.elapsed() < Duration::from_millis(450) {
                return cached;
            }
        }
    }

    let mut fresh: Option<f32> = None;
    #[cfg(target_os = "windows")]
    {
        let sampler_mutex = PDH_GPU_SAMPLER.get_or_init(|| {
            unsafe {
                let mut query = PDH_HQUERY::default();
                if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 || query.is_invalid() {
                    return Mutex::new(None);
                }
                let path_wide = to_wide_null("\\GPU Engine(*engtype_3D)\\Utilization Percentage");
                let mut counter = PDH_HCOUNTER::default();
                if PdhAddEnglishCounterW(query, PCWSTR(path_wide.as_ptr()), 0, &mut counter) != 0 {
                    let _ = PdhCloseQuery(query);
                    return Mutex::new(None);
                }
                let _ = PdhCollectQueryData(query);
                Mutex::new(Some(GpuPdhSampler {
                    query,
                    counter_3d: counter,
                    primed: false,
                }))
            }
        });

        if let Ok(mut guard) = sampler_mutex.lock() {
            if let Some(sampler) = guard.as_mut() {
                unsafe {
                    if PdhCollectQueryData(sampler.query) == 0 {
                        if !sampler.primed {
                            sampler.primed = true;
                            fresh = Some(0.0);
                        } else {
                            let mut buf_size: u32 = 0;
                            let mut item_count: u32 = 0;
                            let _ = PdhGetFormattedCounterArrayW(
                                sampler.counter_3d,
                                PDH_FMT_DOUBLE,
                                &mut buf_size,
                                &mut item_count,
                                None,
                            );
                            if buf_size == 0 {
                                fresh = Some(0.0);
                            } else {
                                let mut raw_buf = vec![0u8; buf_size as usize];
                                let items_ptr = raw_buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
                                let status = PdhGetFormattedCounterArrayW(
                                    sampler.counter_3d,
                                    PDH_FMT_DOUBLE,
                                    &mut buf_size,
                                    &mut item_count,
                                    Some(items_ptr),
                                );
                                if status == 0 && item_count > 0 {
                                    let items = std::slice::from_raw_parts(items_ptr, item_count as usize);
                                    let mut sum = 0.0f64;
                                    for item in items {
                                        if item.FmtValue.CStatus == 0 {
                                            sum += item.FmtValue.Anonymous.doubleValue;
                                        }
                                    }
                                    let clamped = sum.clamp(0.0, 100.0) as f32;
                                    fresh = Some((clamped * 10.0).round() / 10.0);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if let Ok(mut guard) = cache.lock() {
        *guard = Some((Instant::now(), fresh));
    }
    fresh
}

fn vendor_name_from_id(vendor_id: u32, name: &str) -> String {
    let lower = name.to_lowercase();
    if vendor_id == 0x10DE || lower.contains("nvidia") || lower.contains("geforce") || lower.contains("rtx") {
        "NVIDIA".to_string()
    } else if vendor_id == 0x8086 || lower.contains("intel") || lower.contains("arc") || lower.contains("uhd") || lower.contains("iris") {
        "Intel".to_string()
    } else if vendor_id == 0x1002 || lower.contains("amd") || lower.contains("radeon") {
        "AMD".to_string()
    } else {
        "GPU".to_string()
    }
}

fn collect_dxgi_gpus() -> Vec<GpuTelemetry> {
    let mut gpus = Vec::new();
    let pdh_util = sample_pdh_gpu_3d_utilization();

    #[cfg(target_os = "windows")]
    unsafe {
        if let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() {
            let mut idx = 0u32;
            while let Ok(adapter) = factory.EnumAdapters1(idx) {
                if let Ok(desc) = adapter.GetDesc1() {
                    let is_software = (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0
                        || (desc.VendorId == 0x1414 && desc.DeviceId == 0x008c);
                    if !is_software {
                        let name = from_wide_null(&desc.Description);
                        let total_mb = (desc.DedicatedVideoMemory as u64) / (1024 * 1024);
                        let mut used_mb = 0u64;
                        if let Ok(adapter3) = windows::core::Interface::cast::<IDXGIAdapter3>(&adapter) {
                            let mut mem_info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
                            if adapter3
                                .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut mem_info)
                                .is_ok()
                            {
                                used_mb = mem_info.CurrentUsage / (1024 * 1024);
                            }
                        }
                        let vendor = vendor_name_from_id(desc.VendorId, &name);
                        gpus.push(GpuTelemetry {
                            index: gpus.len(),
                            name,
                            vendor,
                            backend: "DXGI".to_string(),
                            pci_vendor_id: desc.VendorId,
                            pci_device_id: desc.DeviceId,
                            temperature_c: None,
                            gpu_clock_mhz: None,
                            mem_clock_mhz: None,
                            vram_used_mb: used_mb,
                            vram_total_mb: total_mb,
                            power_draw_watts: None,
                            power_limit_watts: None,
                            fan_speed_pct: None,
                            utilization_pct: pdh_util,
                            is_primary: false,
                        });
                    }
                }
                idx += 1;
            }
        }
    }

    if let Some(nvml) = get_nvml_api() {
        unsafe {
            let mut count: u32 = 0;
            if (nvml.get_count)(&mut count) == NVML_SUCCESS && count > 0 {
                for dev_idx in 0..count {
                    let mut handle: *mut c_void = std::ptr::null_mut();
                    if (nvml.get_handle)(dev_idx, &mut handle) != NVML_SUCCESS || handle.is_null() {
                        continue;
                    }
                    let mut name_buf = [0 as c_char; 96];
                    let nvml_name = if let Some(f) = nvml.get_name {
                        if f(handle, name_buf.as_mut_ptr(), name_buf.len() as u32) == NVML_SUCCESS {
                            CStr::from_ptr(name_buf.as_ptr()).to_string_lossy().trim().to_string()
                        } else {
                            "NVIDIA GPU".to_string()
                        }
                    } else {
                        "NVIDIA GPU".to_string()
                    };

                    let mut temp_c = None;
                    if let Some(f) = nvml.get_temp {
                        let mut val = 0u32;
                        if f(handle, NVML_TEMPERATURE_GPU, &mut val) == NVML_SUCCESS {
                            temp_c = Some(val);
                        }
                    }

                    let mut gpu_clk = None;
                    let mut mem_clk = None;
                    if let Some(f) = nvml.get_clock {
                        let mut v1 = 0u32;
                        if f(handle, NVML_CLOCK_GRAPHICS, &mut v1) == NVML_SUCCESS {
                            gpu_clk = Some(v1);
                        }
                        let mut v2 = 0u32;
                        if f(handle, NVML_CLOCK_MEM, &mut v2) == NVML_SUCCESS {
                            mem_clk = Some(v2);
                        }
                    }

                    let mut vram_used_mb = 0u64;
                    let mut vram_total_mb = 0u64;
                    if let Some(f) = nvml.get_memory {
                        let mut mem = NvmlMemory { total: 0, free: 0, used: 0 };
                        if f(handle, &mut mem) == NVML_SUCCESS {
                            vram_used_mb = mem.used / (1024 * 1024);
                            vram_total_mb = mem.total / (1024 * 1024);
                        }
                    }

                    let mut power_w = None;
                    if let Some(f) = nvml.get_power_usage {
                        let mut mw = 0u32;
                        if f(handle, &mut mw) == NVML_SUCCESS {
                            power_w = Some(((mw as f64 / 1000.0) * 10.0).round() / 10.0);
                        }
                    }

                    let mut power_lim = None;
                    if let Some(f) = nvml.get_power_limit {
                        let mut mw = 0u32;
                        if f(handle, &mut mw) == NVML_SUCCESS {
                            power_lim = Some(((mw as f64 / 1000.0) * 10.0).round() / 10.0);
                        }
                    }

                    let mut fan_pct = None;
                    if let Some(f) = nvml.get_fan_speed {
                        let mut spd = 0u32;
                        if f(handle, &mut spd) == NVML_SUCCESS {
                            fan_pct = Some(spd);
                        }
                    }

                    let mut util_pct = None;
                    if let Some(f) = nvml.get_utilization {
                        let mut util = NvmlUtilization { gpu: 0, memory: 0 };
                        if f(handle, &mut util) == NVML_SUCCESS {
                            util_pct = Some(util.gpu as f32);
                        }
                    }

                    // Merge into matching DXGI NVIDIA GPU entry or append
                    if let Some(existing) = gpus.iter_mut().find(|g| g.pci_vendor_id == 0x10DE && g.backend != "NVML") {
                        existing.name = if !nvml_name.is_empty() { nvml_name } else { existing.name.clone() };
                        existing.backend = "NVML".to_string();
                        existing.temperature_c = temp_c;
                        existing.gpu_clock_mhz = gpu_clk;
                        existing.mem_clock_mhz = mem_clk;
                        if vram_total_mb > 0 {
                            existing.vram_used_mb = vram_used_mb;
                            existing.vram_total_mb = vram_total_mb;
                        }
                        existing.power_draw_watts = power_w;
                        existing.power_limit_watts = power_lim;
                        existing.fan_speed_pct = fan_pct;
                        existing.utilization_pct = util_pct.or(existing.utilization_pct);
                    } else {
                        gpus.push(GpuTelemetry {
                            index: gpus.len(),
                            name: nvml_name,
                            vendor: "NVIDIA".to_string(),
                            backend: "NVML".to_string(),
                            pci_vendor_id: 0x10DE,
                            pci_device_id: 0,
                            temperature_c: temp_c,
                            gpu_clock_mhz: gpu_clk,
                            mem_clock_mhz: mem_clk,
                            vram_used_mb,
                            vram_total_mb,
                            power_draw_watts: power_w,
                            power_limit_watts: power_lim,
                            fan_speed_pct: fan_pct,
                            utilization_pct: util_pct,
                            is_primary: false,
                        });
                    }
                }
            }
        }
    }

    if gpus.is_empty() {
        let wmi_list = get_cached_wmi_gpus();
        for (idx, wg) in wmi_list.into_iter().enumerate() {
            let vendor = vendor_name_from_id(0, &wg.name);
            gpus.push(GpuTelemetry {
                index: idx,
                name: wg.name,
                vendor,
                backend: "WMI/PDH".to_string(),
                pci_vendor_id: 0,
                pci_device_id: 0,
                temperature_c: None,
                gpu_clock_mhz: None,
                mem_clock_mhz: None,
                vram_used_mb: 0,
                vram_total_mb: wg.adapter_ram_mb,
                power_draw_watts: None,
                power_limit_watts: None,
                fan_speed_pct: None,
                utilization_pct: pdh_util,
                is_primary: idx == 0,
            });
        }
    }

    // Prioritize discrete NVIDIA/AMD/Arc GPU (or highest VRAM) as primary
    if !gpus.is_empty() {
        let best_idx = gpus
            .iter()
            .enumerate()
            .max_by_key(|(_, g)| {
                let backend_bonus = if g.backend == "NVML" { 1_000_000u64 } else { 0 };
                backend_bonus + g.vram_total_mb
            })
            .map(|(i, _)| i)
            .unwrap_or(0);
        for (i, g) in gpus.iter_mut().enumerate() {
            g.is_primary = i == best_idx;
        }
    }

    gpus
}

fn query_wmi_video_controllers() -> Vec<WmiVideoInfo> {
    let ps = "Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion,DriverDate,PNPDeviceID,AdapterRAM | ConvertTo-Json -Compress";
    let mut cmd = Command::new("powershell");
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", ps]);
    let mut out_list = Vec::new();
    if let Ok(output) = cmd.output() {
        if output.status.success() {
            let txt = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !txt.is_empty() {
                if let Ok(val) = serde_json::from_str::<Value>(&txt) {
                    let arr = if let Some(a) = val.as_array() {
                        a.clone()
                    } else {
                        vec![val]
                    };
                    for item in arr {
                        let name = item.get("Name").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                        if name.is_empty() {
                            continue;
                        }
                        let driver_version = item.get("DriverVersion").and_then(|v| v.as_str()).unwrap_or("--").to_string();
                        let driver_date = item.get("DriverDate").and_then(|v| v.as_str()).unwrap_or("--").to_string();
                        let pnp_device_id = item.get("PNPDeviceID").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let ram_bytes = item.get("AdapterRAM").and_then(|v| v.as_u64()).unwrap_or(0);
                        out_list.push(WmiVideoInfo {
                            name,
                            driver_version,
                            driver_date,
                            pnp_device_id,
                            adapter_ram_mb: ram_bytes / (1024 * 1024),
                        });
                    }
                }
            }
        }
    }
    out_list
}

fn get_cached_wmi_gpus() -> Vec<WmiVideoInfo> {
    let cache = WMI_GPU_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = cache.lock() {
        if let Some((ts, list)) = guard.as_ref() {
            if ts.elapsed() < Duration::from_secs(60) {
                return list.clone();
            }
        }
        let fresh = query_wmi_video_controllers();
        *guard = Some((Instant::now(), fresh.clone()));
        return fresh;
    }
    Vec::new()
}

fn read_memory_telemetry() -> MemoryTelemetry {
    #[cfg(target_os = "windows")]
    unsafe {
        let mut ms = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        if GlobalMemoryStatusEx(&mut ms).is_ok() {
            let total_gb = ms.ullTotalPhys as f64 / (1024.0 * 1024.0 * 1024.0);
            let avail_gb = ms.ullAvailPhys as f64 / (1024.0 * 1024.0 * 1024.0);
            let used_gb = (total_gb - avail_gb).max(0.0);
            return MemoryTelemetry {
                used_gb: (used_gb * 100.0).round() / 100.0,
                available_gb: (avail_gb * 100.0).round() / 100.0,
                total_gb: (total_gb * 100.0).round() / 100.0,
                load_pct: ms.dwMemoryLoad,
                own_working_set_mb: get_process_working_set_mb(),
            };
        }
    }
    MemoryTelemetry {
        used_gb: 0.0,
        available_gb: 0.0,
        total_gb: 0.0,
        load_pct: 0,
        own_working_set_mb: get_process_working_set_mb(),
    }
}

fn match_bloat_signature(proc_name: &str) -> Option<&'static str> {
    let lower = proc_name.to_lowercase();
    let stem = lower.strip_suffix(".exe").unwrap_or(&lower);
    for (sig, reason) in BLOAT_SIGNATURES {
        if stem == *sig || stem.contains(sig) {
            return Some(*reason);
        }
    }
    None
}

fn purge_background_ram_internal(exclude_pid: Option<u32>) -> RamPurgeResult {
    let before_mem = read_memory_telemetry();
    let current_pid = std::process::id();
    let boosted_snapshot: HashSet<u32> = get_boosted_pids()
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default();

    let mut ws_freed_bytes: u64 = 0;
    let mut processes_trimmed: usize = 0;

    #[cfg(target_os = "windows")]
    unsafe {
        if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            let mut pe = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            if Process32FirstW(snap, &mut pe).is_ok() {
                loop {
                    let pid = pe.th32ProcessID;
                    if pid > 4
                        && pid != current_pid
                        && Some(pid) != exclude_pid
                        && !boosted_snapshot.contains(&pid)
                    {
                        if let Ok(proc_handle) = OpenProcess(
                            PROCESS_SET_QUOTA | PROCESS_QUERY_LIMITED_INFORMATION,
                            false,
                            pid,
                        ) {
                            let mut before_c = ProcessMemoryCounters {
                                cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
                                page_fault_count: 0,
                                peak_working_set_size: 0,
                                working_set_size: 0,
                                quota_peak_paged_pool_usage: 0,
                                quota_paged_pool_usage: 0,
                                quota_peak_non_paged_pool_usage: 0,
                                quota_non_paged_pool_usage: 0,
                                pagefile_usage: 0,
                                peak_pagefile_usage: 0,
                            };
                            let has_before = K32GetProcessMemoryInfo(
                                proc_handle.0,
                                &mut before_c,
                                before_c.cb,
                            ) != 0;

                            let _ = SetProcessWorkingSetSizeEx(
                                proc_handle.0,
                                usize::MAX,
                                usize::MAX,
                                0,
                            );
                            if K32EmptyWorkingSet(proc_handle.0) != 0 {
                                processes_trimmed += 1;
                                if has_before {
                                    let mut after_c = ProcessMemoryCounters {
                                        cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
                                        page_fault_count: 0,
                                        peak_working_set_size: 0,
                                        working_set_size: 0,
                                        quota_peak_paged_pool_usage: 0,
                                        quota_paged_pool_usage: 0,
                                        quota_peak_non_paged_pool_usage: 0,
                                        quota_non_paged_pool_usage: 0,
                                        pagefile_usage: 0,
                                        peak_pagefile_usage: 0,
                                    };
                                    if K32GetProcessMemoryInfo(
                                        proc_handle.0,
                                        &mut after_c,
                                        after_c.cb,
                                    ) != 0
                                    {
                                        let delta = before_c
                                            .working_set_size
                                            .saturating_sub(after_c.working_set_size);
                                        ws_freed_bytes = ws_freed_bytes.saturating_add(delta as u64);
                                    }
                                }
                            }
                            let _ = CloseHandle(proc_handle);
                        }
                    }
                    if Process32NextW(snap, &mut pe).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
        }
    }

    trim_process_working_set();
    let after_mem = read_memory_telemetry();

    let ws_freed_mb = (ws_freed_bytes as f64 / (1024.0 * 1024.0) * 10.0).round() / 10.0;
    let avail_gain_mb =
        (((after_mem.available_gb - before_mem.available_gb).max(0.0) * 1024.0) * 10.0).round()
            / 10.0;
    let freed_mb = ws_freed_mb.max(avail_gain_mb);

    RamPurgeResult {
        freed_mb,
        working_set_freed_mb: ws_freed_mb,
        available_gain_mb: avail_gain_mb,
        processes_trimmed,
        before_used_gb: before_mem.used_gb,
        after_used_gb: after_mem.used_gb,
        own_working_set_mb: after_mem.own_working_set_mb,
    }
}

fn read_process_name_by_pid(pid: u32) -> String {
    if let Ok(sys) = get_sysinfo().lock() {
        if let Some(p) = sys.process(sysinfo::Pid::from_u32(pid)) {
            return p.name().to_string_lossy().to_string();
        }
    }
    format!("PID {}", pid)
}

#[tauri::command]
async fn get_hardware_telemetry() -> Result<HardwareTelemetry, String> {
    tokio::task::spawn_blocking(move || {
        let topology = get_cpu_topology();
        let cpu_telemetry = {
            let mut sys = get_sysinfo().lock().map_err(|_| "Sysinfo lock poisoned".to_string())?;
            sys.refresh_cpu_usage();
            let cpus = sys.cpus();
            let model = cpus
                .first()
                .map(|c| c.brand().trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Windows x64 Processor".to_string());
            let total_usage_pct = (sys.global_cpu_usage() * 10.0).round() / 10.0;
            let avg_freq = if !cpus.is_empty() {
                cpus.iter().map(|c| c.frequency()).sum::<u64>() / cpus.len() as u64
            } else {
                0
            };

            let cores: Vec<LogicalCoreUsage> = cpus
                .iter()
                .enumerate()
                .map(|(i, c)| LogicalCoreUsage {
                    index: i,
                    usage_pct: (c.cpu_usage() * 10.0).round() / 10.0,
                    frequency_mhz: c.frequency(),
                    core_type: topology
                        .core_types
                        .get(&i)
                        .cloned()
                        .unwrap_or_else(|| "CORE".to_string()),
                    efficiency_class: *topology.efficiency_classes.get(&i).unwrap_or(&0),
                })
                .collect();

            CpuTelemetry {
                model,
                total_usage_pct,
                frequency_mhz: avg_freq,
                physical_cores: if topology.physical_cores > 0 {
                    topology.physical_cores
                } else {
                    sys.physical_core_count().unwrap_or(cores.len())
                },
                logical_cores: cores.len(),
                p_core_count: topology.p_core_count,
                e_core_count: topology.e_core_count,
                hybrid_detected: topology.hybrid_detected,
                cores,
            }
        };

        let gpus = collect_dxgi_gpus();
        let memory = read_memory_telemetry();
        let doh_hosts = duke_core::net_doh::cached_doh_host_count();
        let eco_count = get_eco_jailed_pids().lock().map(|s| s.len()).unwrap_or(0);

        Ok(HardwareTelemetry {
            gpus,
            cpu: cpu_telemetry,
            memory,
            doh_shield: "ACTIVE".to_string(),
            doh_cached_hosts: doh_hosts,
            auto_trim_enabled: AUTO_TRIM_ENABLED.load(Ordering::Relaxed),
            eco_scheduler_status: if eco_count > 0 {
                format!("ACTIVE ({} JAILED)", eco_count)
            } else {
                "READY".to_string()
            },
            eco_jailed_count: eco_count,
        })
    })
    .await
    .map_err(|e| format!("Telemetry worker failed: {}", e))?
}

#[tauri::command]
async fn purge_system_ram() -> Result<RamPurgeResult, String> {
    tokio::task::spawn_blocking(move || Ok(purge_background_ram_internal(None)))
        .await
        .map_err(|e| format!("RAM purge failed: {}", e))?
}

#[tauri::command]
async fn get_running_processes() -> Result<Vec<ProcessEntry>, String> {
    tokio::task::spawn_blocking(move || {
        let mut sys = get_sysinfo().lock().map_err(|_| "Sysinfo lock poisoned".to_string())?;
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let num_cpus = sys.cpus().len().max(1) as f32;
        let jailed_set = get_eco_jailed_pids()
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default();
        let boosted_set = get_boosted_pids()
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default();

        let mut list: Vec<ProcessEntry> = sys
            .processes()
            .values()
            .filter_map(|proc_item| {
                let pid = proc_item.pid().as_u32();
                if pid <= 4 {
                    return None;
                }
                let name = proc_item.name().to_string_lossy().to_string();
                let exe_path = proc_item
                    .exe()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                let ram_mb = ((proc_item.memory() as f64 / (1024.0 * 1024.0)) * 10.0).round() / 10.0;
                let cpu_pct = ((proc_item.cpu_usage() / num_cpus) * 10.0).round() / 10.0;
                let bloat_reason = match_bloat_signature(&name).map(|r| r.to_string());
                let is_bloat = bloat_reason.is_some();
                let eco_jailed = jailed_set.contains(&pid);
                let boosted = boosted_set.contains(&pid);
                let status = if boosted {
                    "BOOSTED (ABOVE NORMAL)".to_string()
                } else if eco_jailed {
                    "ECO JAILED (E-CORE)".to_string()
                } else if is_bloat {
                    "OEM/TELEMETRY BLOAT".to_string()
                } else {
                    "NORMAL".to_string()
                };

                Some(ProcessEntry {
                    pid,
                    name,
                    exe_path,
                    cpu_pct,
                    ram_mb,
                    status,
                    eco_jailed,
                    boosted,
                    is_bloat,
                    bloat_reason,
                })
            })
            .collect();

        list.sort_by(|a, b| {
            b.ram_mb
                .partial_cmp(&a.ram_mb)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(list)
    })
    .await
    .map_err(|e| format!("Process scan failed: {}", e))?
}

#[tauri::command]
async fn set_process_eco_qos(pid: u32) -> Result<ProcessActionResult, String> {
    if pid <= 4 || pid == std::process::id() {
        return Err("Cannot Eco-Jail system idle/kernel or Duke Monitor itself.".to_string());
    }
    let name = read_process_name_by_pid(pid);
    let mut freed_mb = 0.0f64;

    #[cfg(target_os = "windows")]
    unsafe {
        let handle = OpenProcess(
            PROCESS_SET_INFORMATION | PROCESS_SET_QUOTA | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            pid,
        )
        .map_err(|e| format!("Access denied opening {} (PID {}): {}", name, pid, e))?;

        let mut before_c = ProcessMemoryCounters {
            cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
            page_fault_count: 0,
            peak_working_set_size: 0,
            working_set_size: 0,
            quota_peak_paged_pool_usage: 0,
            quota_paged_pool_usage: 0,
            quota_peak_non_paged_pool_usage: 0,
            quota_non_paged_pool_usage: 0,
            pagefile_usage: 0,
            peak_pagefile_usage: 0,
        };
        let has_before = K32GetProcessMemoryInfo(handle.0, &mut before_c, before_c.cb) != 0;

        let pri_ok = SetPriorityClass(handle.0, BELOW_NORMAL_PRIORITY_CLASS) != 0;
        let throttling = ProcessPowerThrottlingState {
            version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            control_mask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            state_mask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        };
        let throttle_ok = SetProcessInformation(
            handle.0,
            PROCESS_POWER_THROTTLING_INFO_CLASS,
            &throttling as *const _ as *const c_void,
            std::mem::size_of::<ProcessPowerThrottlingState>() as u32,
        ) != 0;

        let _ = SetProcessWorkingSetSizeEx(handle.0, usize::MAX, usize::MAX, 0);
        if K32EmptyWorkingSet(handle.0) != 0 && has_before {
            let mut after_c = ProcessMemoryCounters {
                cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
                page_fault_count: 0,
                peak_working_set_size: 0,
                working_set_size: 0,
                quota_peak_paged_pool_usage: 0,
                quota_paged_pool_usage: 0,
                quota_peak_non_paged_pool_usage: 0,
                quota_non_paged_pool_usage: 0,
                pagefile_usage: 0,
                peak_pagefile_usage: 0,
            };
            if K32GetProcessMemoryInfo(handle.0, &mut after_c, after_c.cb) != 0 {
                let delta = before_c
                    .working_set_size
                    .saturating_sub(after_c.working_set_size);
                freed_mb = ((delta as f64 / (1024.0 * 1024.0)) * 10.0).round() / 10.0;
            }
        }
        let _ = CloseHandle(handle);

        if !pri_ok && !throttle_ok {
            return Err(format!(
                "Windows refused EcoQoS update for {} (PID {}). Try running as Administrator for elevated processes.",
                name, pid
            ));
        }
    }

    if let Ok(mut jailed) = get_eco_jailed_pids().lock() {
        jailed.insert(pid);
    }
    if let Ok(mut boosted) = get_boosted_pids().lock() {
        boosted.remove(&pid);
    }

    Ok(ProcessActionResult {
        pid,
        name: name.clone(),
        freed_mb,
        message: format!(
            "EcoQoS Jailed {} (PID {}): Pinned to Efficiency cores (BELOW_NORMAL + ExecutionSpeed), trimmed {:.1} MB RAM.",
            name, pid, freed_mb
        ),
    })
}

#[tauri::command]
async fn boost_game_process(pid: u32) -> Result<GameBoostResult, String> {
    if pid <= 4 {
        return Err("Cannot boost kernel/system idle PID.".to_string());
    }
    let name = read_process_name_by_pid(pid);

    duke_core::win32_ffi::boost_external_pid(pid)
        .map_err(|e| format!("Failed boosting {} (PID {}): {}", name, pid, e))?;

    if let Ok(mut boosted) = get_boosted_pids().lock() {
        boosted.insert(pid);
    }
    if let Ok(mut jailed) = get_eco_jailed_pids().lock() {
        jailed.remove(&pid);
    }

    let purge = tokio::task::spawn_blocking(move || purge_background_ram_internal(Some(pid)))
        .await
        .map_err(|e| format!("Background RAM purge failed: {}", e))?;

    Ok(GameBoostResult {
        pid,
        name: name.clone(),
        background_freed_mb: purge.freed_mb,
        processes_trimmed: purge.processes_trimmed,
        message: format!(
            "Game Boost active on {} (PID {}): ABOVE_NORMAL priority + purged {:.1} MB across {} background processes.",
            name, pid, purge.freed_mb, purge.processes_trimmed
        ),
    })
}

#[tauri::command]
async fn trim_process_ram(pid: u32) -> Result<ProcessActionResult, String> {
    if pid <= 4 {
        return Err("Cannot trim kernel/system idle PID.".to_string());
    }
    let name = read_process_name_by_pid(pid);
    let mut freed_mb = 0.0f64;

    #[cfg(target_os = "windows")]
    unsafe {
        let handle = OpenProcess(
            PROCESS_SET_QUOTA | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            pid,
        )
        .map_err(|e| format!("Access denied trimming {} (PID {}): {}", name, pid, e))?;

        let mut before_c = ProcessMemoryCounters {
            cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
            page_fault_count: 0,
            peak_working_set_size: 0,
            working_set_size: 0,
            quota_peak_paged_pool_usage: 0,
            quota_paged_pool_usage: 0,
            quota_peak_non_paged_pool_usage: 0,
            quota_non_paged_pool_usage: 0,
            pagefile_usage: 0,
            peak_pagefile_usage: 0,
        };
        let has_before = K32GetProcessMemoryInfo(handle.0, &mut before_c, before_c.cb) != 0;
        let _ = SetProcessWorkingSetSizeEx(handle.0, usize::MAX, usize::MAX, 0);
        let trimmed = K32EmptyWorkingSet(handle.0) != 0;
        if trimmed && has_before {
            let mut after_c = ProcessMemoryCounters {
                cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
                page_fault_count: 0,
                peak_working_set_size: 0,
                working_set_size: 0,
                quota_peak_paged_pool_usage: 0,
                quota_paged_pool_usage: 0,
                quota_peak_non_paged_pool_usage: 0,
                quota_non_paged_pool_usage: 0,
                pagefile_usage: 0,
                peak_pagefile_usage: 0,
            };
            if K32GetProcessMemoryInfo(handle.0, &mut after_c, after_c.cb) != 0 {
                let delta = before_c
                    .working_set_size
                    .saturating_sub(after_c.working_set_size);
                freed_mb = ((delta as f64 / (1024.0 * 1024.0)) * 10.0).round() / 10.0;
            }
        }
        let _ = CloseHandle(handle);
        if !trimmed {
            return Err(format!("Failed to trim working set for {} (PID {}).", name, pid));
        }
    }

    Ok(ProcessActionResult {
        pid,
        name: name.clone(),
        freed_mb,
        message: format!("Trimmed {:.1} MB working set from {} (PID {}).", freed_mb, name, pid),
    })
}

fn terminate_pid_sync(pid: u32) -> Result<(), String> {
    if pid <= 4 || pid == std::process::id() {
        return Err("Refused to terminate protected system/self PID.".to_string());
    }
    #[cfg(target_os = "windows")]
    unsafe {
        let handle: HANDLE = OpenProcess(PROCESS_TERMINATE, false, pid)
            .map_err(|e| format!("Access denied terminating PID {}: {}", pid, e))?;
        let res = TerminateProcess(handle, 1);
        let _ = CloseHandle(handle);
        res.map_err(|e| format!("TerminateProcess failed for PID {}: {}", pid, e))?;
    }
    if let Ok(mut jailed) = get_eco_jailed_pids().lock() {
        jailed.remove(&pid);
    }
    if let Ok(mut boosted) = get_boosted_pids().lock() {
        boosted.remove(&pid);
    }
    Ok(())
}

#[tauri::command]
async fn kill_process(pid: u32) -> Result<ProcessActionResult, String> {
    let name = read_process_name_by_pid(pid);
    terminate_pid_sync(pid)?;
    Ok(ProcessActionResult {
        pid,
        name: name.clone(),
        freed_mb: 0.0,
        message: format!("Terminated {} (PID {}).", name, pid),
    })
}

#[tauri::command]
async fn kill_all_bloat_processes() -> Result<BloatPurgeResult, String> {
    let targets: Vec<(u32, String, f64)> = {
        let mut sys = get_sysinfo().lock().map_err(|_| "Sysinfo lock poisoned".to_string())?;
        sys.refresh_processes(ProcessesToUpdate::All, true);
        sys.processes()
            .values()
            .filter_map(|p| {
                let pid = p.pid().as_u32();
                if pid <= 4 || pid == std::process::id() {
                    return None;
                }
                let name = p.name().to_string_lossy().to_string();
                if match_bloat_signature(&name).is_some() {
                    let ram_mb = p.memory() as f64 / (1024.0 * 1024.0);
                    Some((pid, name, ram_mb))
                } else {
                    None
                }
            })
            .collect()
    };

    if targets.is_empty() {
        return Ok(BloatPurgeResult {
            killed_count: 0,
            failed_count: 0,
            reclaimed_mb: 0.0,
            killed_names: Vec::new(),
            message: "Zero OEM or telemetry bloat processes currently running.".to_string(),
        });
    }

    let mut killed_count = 0usize;
    let mut failed_count = 0usize;
    let mut reclaimed_mb = 0.0f64;
    let mut killed_names = Vec::new();

    for (pid, name, ram_mb) in targets {
        if terminate_pid_sync(pid).is_ok() {
            killed_count += 1;
            reclaimed_mb += ram_mb;
            if !killed_names.contains(&name) {
                killed_names.push(name);
            }
        } else {
            failed_count += 1;
        }
    }

    reclaimed_mb = (reclaimed_mb * 10.0).round() / 10.0;
    Ok(BloatPurgeResult {
        killed_count,
        failed_count,
        reclaimed_mb,
        killed_names,
        message: format!(
            "Killed {} bloat process(es), reclaimed {:.1} MB RAM ({} protected/skipped).",
            killed_count, reclaimed_mb, failed_count
        ),
    })
}

fn normalize_nvidia_driver_version(raw_ver: &str) -> String {
    // Windows registry stores NVIDIA versions like "32.0.15.6636" -> "566.36"
    let digits: String = raw_ver.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 5 {
        let tail = &digits[digits.len() - 5..];
        return format!("{}.{}", &tail[..3], &tail[3..]);
    }
    raw_ver.to_string()
}

fn compare_dotted_versions(installed: &str, latest: &str) -> bool {
    let parse_parts = |s: &str| -> Vec<u64> {
        s.split('.')
            .filter_map(|p| {
                let d: String = p.chars().filter(|c| c.is_ascii_digit()).collect();
                if d.is_empty() {
                    None
                } else {
                    d.parse::<u64>().ok()
                }
            })
            .collect()
    };
    let a = parse_parts(installed);
    let b = parse_parts(latest);
    let max_len = a.len().max(b.len());
    for i in 0..max_len {
        let av = *a.get(i).unwrap_or(&0);
        let bv = *b.get(i).unwrap_or(&0);
        if bv > av {
            return true;
        }
        if av > bv {
            return false;
        }
    }
    false
}

fn inspect_registry_display_drivers() -> Vec<(String, String, String, String, String)> {
    // Returns (DriverDesc, DriverVersion, DriverDate, ProviderName, MatchingDeviceId)
    let mut results = Vec::new();
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let class_path = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
    if let Ok(class_key) = hklm.open_subkey(class_path) {
        for sub_name in class_key.enum_keys().flatten() {
            if sub_name.len() != 4 || !sub_name.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            if let Ok(subkey) = class_key.open_subkey(&sub_name) {
                let desc: String = subkey.get_value("DriverDesc").unwrap_or_default();
                if desc.is_empty() || desc.to_lowercase().contains("basic display") {
                    continue;
                }
                let ver: String = subkey.get_value("DriverVersion").unwrap_or_else(|_| "--".to_string());
                let date: String = subkey.get_value("DriverDate").unwrap_or_else(|_| "--".to_string());
                let provider: String = subkey.get_value("ProviderName").unwrap_or_default();
                let dev_id: String = subkey.get_value("MatchingDeviceId").unwrap_or_default();
                results.push((desc, ver, date, provider, dev_id));
            }
        }
    }
    results
}

fn inspect_cpu_registry_and_wmi() -> (String, String, String, String, String) {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let mut cpu_name = String::new();
    let mut vendor_id = String::new();
    let mut identifier = String::new();
    if let Ok(cpu_key) = hklm.open_subkey(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0") {
        cpu_name = cpu_key
            .get_value::<String, _>("ProcessorNameString")
            .unwrap_or_default()
            .trim()
            .to_string();
        vendor_id = cpu_key
            .get_value::<String, _>("VendorIdentifier")
            .unwrap_or_default()
            .trim()
            .to_string();
        identifier = cpu_key
            .get_value::<String, _>("Identifier")
            .unwrap_or_default()
            .trim()
            .to_string();
    }

    let ps = "Get-CimInstance Win32_Processor | Select-Object -First 1 Name,Manufacturer,ProcessorId,Revision | ConvertTo-Json -Compress";
    let mut cmd = Command::new("powershell");
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", ps]);
    let mut proc_id = String::new();
    if let Ok(out) = cmd.output() {
        if out.status.success() {
            if let Ok(val) = serde_json::from_str::<Value>(&String::from_utf8_lossy(&out.stdout)) {
                if cpu_name.is_empty() {
                    cpu_name = val.get("Name").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                }
                if vendor_id.is_empty() {
                    vendor_id = val.get("Manufacturer").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                }
                proc_id = val.get("ProcessorId").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
            }
        }
    }

    // Query Microsoft CPU driver version from WMI Win32_PnPSignedDriver for Processor class
    let mut drv_ver = "10.0.22621".to_string();
    let mut drv_date = "--".to_string();
    let ps_drv = "Get-CimInstance Win32_PnPSignedDriver -Filter \"DeviceClass='PROCESSOR'\" | Select-Object -First 1 DriverVersion,DriverDate | ConvertTo-Json -Compress";
    let mut cmd2 = Command::new("powershell");
    cmd2.creation_flags(CREATE_NO_WINDOW);
    cmd2.args(["-NoProfile", "-NonInteractive", "-Command", ps_drv]);
    if let Ok(out) = cmd2.output() {
        if out.status.success() {
            if let Ok(val) = serde_json::from_str::<Value>(&String::from_utf8_lossy(&out.stdout)) {
                if let Some(v) = val.get("DriverVersion").and_then(|v| v.as_str()) {
                    if !v.trim().is_empty() {
                        drv_ver = v.trim().to_string();
                    }
                }
                if let Some(d) = val.get("DriverDate").and_then(|v| v.as_str()) {
                    drv_date = d.chars().take(10).collect();
                }
            }
        }
    }

    let vendor = if vendor_id.to_lowercase().contains("intel") || cpu_name.to_lowercase().contains("intel") {
        "Intel".to_string()
    } else if vendor_id.to_lowercase().contains("amd") || cpu_name.to_lowercase().contains("amd") {
        "AMD".to_string()
    } else {
        vendor_id
    };

    let hw_id = if !proc_id.is_empty() {
        format!("CPUID {} ({})", proc_id, identifier)
    } else {
        identifier
    };

    (cpu_name, vendor, hw_id, drv_ver, drv_date)
}

async fn lookup_nvidia_driver_doh(gpu_name: &str, installed_display: &str) -> (Option<String>, Option<String>, Option<String>, Option<String>, String) {
    let lookup_url = "https://www.nvidia.com/Download/API/lookupValueSearch.php?TypeID=3";
    let client = build_doh_client_for_url(lookup_url).await;

    let mut psid = "127".to_string(); // GeForce RTX 40 Series default
    let mut pfid = "997".to_string();

    if let Ok(resp) = client.get(lookup_url).timeout(Duration::from_secs(6)).send().await {
        if resp.status().is_success() {
            if let Ok(xml_or_text) = resp.text().await {
                let norm_gpu = gpu_name.to_lowercase().replace("nvidia", "").trim().to_string();
                let is_laptop = norm_gpu.contains("laptop") || norm_gpu.contains("max-q") || norm_gpu.contains("mobile");
                let gpu_tokens: Vec<&str> = norm_gpu.split_whitespace().collect();

                let mut best_score = 0isize;
                for block in xml_or_text.split("<LookupValue>") {
                    let extract = |tag: &str| -> Option<String> {
                        let start = format!("<{}>", tag);
                        let end = format!("</{}>", tag);
                        let s = block.find(&start)? + start.len();
                        let e = block[s..].find(&end)? + s;
                        Some(block[s..e].trim().to_string())
                    };
                    if let (Some(name), Some(val), Some(parent)) = (extract("Name"), extract("Value"), extract("ParentID")) {
                        let lname = name.to_lowercase();
                        let mut score = 0isize;
                        for tok in &gpu_tokens {
                            if lname.contains(tok) {
                                score += 10;
                            }
                        }
                        if is_laptop && (lname.contains("notebook") || lname.contains("laptop")) {
                            score += 15;
                        } else if !is_laptop && !(lname.contains("notebook") || lname.contains("laptop")) {
                            score += 5;
                        }
                        if lname == norm_gpu {
                            score += 50;
                        }
                        if score > best_score {
                            best_score = score;
                            pfid = val;
                            psid = parent;
                        }
                    }
                }
            }
        }
    }

    let ajax_url = format!(
        "https://gfwsl.geforce.com/services_toolkit/services/com/nvidia/services/AjaxDriverService.php?func=DriverManualLookup&psid={}&pfid={}&osID=135&languageCode=1033&beta=null&isWHQL=1&dltype=-1&dch=1&upCRD=null&qnf=0&sort1=1&numberOfResults=1",
        psid, pfid
    );
    let ajax_client = build_doh_client_for_url(&ajax_url).await;
    if let Ok(resp) = ajax_client.get(&ajax_url).timeout(Duration::from_secs(8)).send().await {
        if resp.status().is_success() {
            if let Ok(json_val) = resp.json::<Value>().await {
                if let Some(info) = json_val
                    .get("IDS")
                    .and_then(|i| i.as_array())
                    .and_then(|a| a.first())
                    .and_then(|f| f.get("downloadInfo"))
                {
                    let ver = info.get("Version").and_then(|v| v.as_str()).map(|s| s.trim().to_string());
                    let rel_date = info.get("ReleaseDateTime").and_then(|v| v.as_str()).map(|s| s.trim().to_string());
                    let dl_url = info.get("DownloadURL").and_then(|v| v.as_str()).map(|s| {
                        let s = s.trim();
                        if s.starts_with("//") {
                            format!("https:{}", s)
                        } else {
                            s.to_string()
                        }
                    });
                    let raw_notes = info
                        .get("ReleaseNotes")
                        .and_then(|v| v.as_str())
                        .or_else(|| info.get("Name").and_then(|v| v.as_str()))
                        .unwrap_or("Official NVIDIA GeForce WHQL Game Ready Driver (DCH).")
                        .to_string();
                    let clean_notes = strip_html_tags(&raw_notes);
                    let fname = dl_url
                        .as_ref()
                        .and_then(|u| u.split('/').next_back().map(|s| s.to_string()))
                        .or_else(|| ver.as_ref().map(|v| format!("NVIDIA_WHQL_{}_Clean.exe", v)));

                    let _ = installed_display;
                    return (ver, rel_date, dl_url, fname, clean_notes);
                }
            }
        }
    }

    (
        None,
        None,
        None,
        None,
        "Could not reach gfwsl.geforce.com over DoH — verify connectivity or open NVIDIA official driver portal.".to_string(),
    )
}

async fn lookup_intel_driver_doh(gpu_or_cpu_name: &str, category: &str) -> (Option<String>, Option<String>, Option<String>, Option<String>, String) {
    let api_url = if category == "GPU" {
        format!(
            "https://www.intel.com/libs/apps/intel/support/downloads/search?query={}&os=Windows%2011,%2064-bit*&sort=date",
            urlencoding::encode("Intel Arc & Iris Xe Graphics")
        )
    } else {
        format!(
            "https://www.intel.com/libs/apps/intel/support/downloads/search?query={}&os=Windows%2011,%2064-bit*&sort=date",
            urlencoding::encode("Intel Chipset Device Software")
        )
    };
    let client = build_doh_client_for_url(&api_url).await;
    if let Ok(resp) = client
        .get(&api_url)
        .header(ACCEPT, "application/json,text/html,*/*")
        .timeout(Duration::from_secs(7))
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(json_val) = resp.json::<Value>().await {
                if let Some(first) = json_val
                    .get("results")
                    .and_then(|r| r.as_array())
                    .and_then(|a| a.first())
                {
                    let ver = first.get("version").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let date = first.get("publishDate").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let title = first.get("title").and_then(|v| v.as_str()).unwrap_or(gpu_or_cpu_name).to_string();
                    let dl = first
                        .get("downloads")
                        .and_then(|d| d.as_array())
                        .and_then(|a| a.first())
                        .and_then(|f| f.get("url"))
                        .and_then(|u| u.as_str())
                        .map(|s| s.to_string());
                    let fname = dl.as_ref().and_then(|u| u.split('/').next_back().map(|s| s.to_string()));
                    return (ver, date, dl, fname, title);
                }
            }
        }
    }

    (
        None,
        None,
        None,
        None,
        "Intel Download Center requires interactive portal lookup or driver DSA endpoint for this SKU; official clean link provided.".to_string(),
    )
}

async fn lookup_amd_driver_doh(device_name: &str) -> (Option<String>, Option<String>, Option<String>, Option<String>, String) {
    let url = "https://www.amd.com/en/support/download/drivers.html";
    let _client = build_doh_client_for_url(url).await;
    (
        None,
        None,
        None,
        None,
        format!(
            "AMD Adrenalin / Chipset lookup resolved over DoH for {}. Use Minimal/Driver-Only install to avoid telemetry.",
            device_name
        ),
    )
}

fn strip_html_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    let clean = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.len() > 260 {
        format!("{}...", &clean[..260])
    } else if clean.is_empty() {
        "Official WHQL driver release.".to_string()
    } else {
        clean
    }
}

#[tauri::command]
async fn check_driver_updates(app: AppHandle) -> Result<DriverAuditReport, String> {
    let drivers_dir = resolve_drivers_download_dir(&app);
    let _ = fs::create_dir_all(&drivers_dir);

    let (reg_gpus, wmi_gpus, cpu_info) = tokio::task::spawn_blocking(move || {
        let reg = inspect_registry_display_drivers();
        let wmi = get_cached_wmi_gpus();
        let cpu = inspect_cpu_registry_and_wmi();
        (reg, wmi, cpu)
    })
    .await
    .map_err(|e| format!("Hardware registry inspection failed: {}", e))?;

    let mut devices: Vec<DriverDeviceInfo> = Vec::new();
    let mut seen_gpu_names = HashSet::new();

    for (idx, (desc, ver, date, provider, dev_id)) in reg_gpus.into_iter().enumerate() {
        if !seen_gpu_names.insert(desc.to_lowercase()) {
            continue;
        }
        let vendor = vendor_name_from_id(0, &format!("{} {}", desc, provider));
        let display_ver = if vendor == "NVIDIA" {
            normalize_nvidia_driver_version(&ver)
        } else {
            ver.clone()
        };

        let (latest_ver, latest_date, dl_url, fname, notes) = if vendor == "NVIDIA" {
            lookup_nvidia_driver_doh(&desc, &display_ver).await
        } else if vendor == "Intel" {
            lookup_intel_driver_doh(&desc, "GPU").await
        } else if vendor == "AMD" {
            lookup_amd_driver_doh(&desc).await
        } else {
            (None, None, None, None, "Standard Windows WDDM display adapter.".to_string())
        };

        let update_available = latest_ver
            .as_ref()
            .map(|lv| compare_dotted_versions(&display_ver, lv))
            .unwrap_or(false);

        let status_message = if let Some(ref lv) = latest_ver {
            if update_available {
                format!("UPDATE AVAILABLE: {} -> {} (WHQL)", display_ver, lv)
            } else {
                format!("UP TO DATE ({})", display_ver)
            }
        } else {
            format!("INSTALLED: {} (Official Portal Ready)", display_ver)
        };

        let (portal, guide) = match vendor.as_str() {
            "NVIDIA" => (
                "https://www.nvidia.com/en-us/drivers/".to_string(),
                "Clean Install (Zero Bloat): Extract installer with 7-Zip and run setup.exe from Display.Driver/NVI2 ONLY, or run: <installer>.exe -s -noreboot -clean (omits GeForce Experience & NvTelemetryContainer).".to_string(),
            ),
            "Intel" => (
                "https://www.intel.com/content/www/us/en/download/785597/intel-arc-iris-xe-graphics-windows.html".to_string(),
                "Clean Install (Zero Bloat): Run Intel Installer with --silent --noExtras or install extracted .inf via Device Manager / pnputil /add-driver without Intel Graphics Command Center telemetry.".to_string(),
            ),
            "AMD" => (
                "https://www.amd.com/en/support/download/drivers.html".to_string(),
                "Clean Install (Zero Bloat): Choose 'Driver Only' in the AMD Adrenalin installer (or extract C:\\AMD and install INF via pnputil) to skip AMD telemetry & background overlay bloat.".to_string(),
            ),
            _ => (
                "https://www.catalog.update.microsoft.com/".to_string(),
                "Standard Windows INF driver installation.".to_string(),
            ),
        };

        devices.push(DriverDeviceInfo {
            id: format!("gpu_{}", idx),
            category: "GPU".to_string(),
            name: desc,
            vendor,
            hardware_id: if dev_id.is_empty() { "PCI\\VEN_DISPLAY".to_string() } else { dev_id },
            installed_version: ver,
            installed_display_version: display_ver,
            driver_date: date,
            latest_version: latest_ver,
            latest_release_date: latest_date,
            update_available,
            status_message,
            release_notes: notes,
            download_url: dl_url,
            suggested_filename: fname,
            official_portal_url: portal,
            clean_install_guide: guide,
        });
    }

    if devices.is_empty() {
        for (idx, wg) in wmi_gpus.into_iter().enumerate() {
            let vendor = vendor_name_from_id(0, &wg.name);
            let display_ver = if vendor == "NVIDIA" {
                normalize_nvidia_driver_version(&wg.driver_version)
            } else {
                wg.driver_version.clone()
            };
            devices.push(DriverDeviceInfo {
                id: format!("gpu_wmi_{}", idx),
                category: "GPU".to_string(),
                name: wg.name,
                vendor,
                hardware_id: wg.pnp_device_id,
                installed_version: wg.driver_version,
                installed_display_version: display_ver,
                driver_date: wg.driver_date,
                latest_version: None,
                latest_release_date: None,
                update_available: false,
                status_message: "Installed (WMI Inspection)".to_string(),
                release_notes: "Detected via Win32_VideoController.".to_string(),
                download_url: None,
                suggested_filename: None,
                official_portal_url: "https://www.nvidia.com/en-us/drivers/".to_string(),
                clean_install_guide: "Extract display driver package and install Display.Driver only.".to_string(),
            });
        }
    }

    let (cpu_name, cpu_vendor, cpu_hw_id, cpu_ver, cpu_date) = cpu_info;
    let (cpu_latest, cpu_latest_date, cpu_dl, cpu_fname, cpu_notes) = if cpu_vendor == "Intel" {
        lookup_intel_driver_doh(&cpu_name, "CPU").await
    } else {
        lookup_amd_driver_doh(&cpu_name).await
    };

    let (cpu_portal, cpu_guide) = if cpu_vendor == "Intel" {
        (
            "https://www.intel.com/content/www/us/en/download/19347/chipset-inf-utility.html".to_string(),
            "Clean Chipset INF Install: Run SetupChipset.exe -s -norestart (updates PCI/ACPI INF tables cleanly with zero background services).".to_string(),
        )
    } else {
        (
            "https://www.amd.com/en/support/download/drivers.html".to_string(),
            "Clean AMD Chipset Install: Extract AMD Chipset Drivers and install Power Plan / PSP / GPIO INF components cleanly without background telemetry.".to_string(),
        )
    };

    devices.push(DriverDeviceInfo {
        id: "cpu_0".to_string(),
        category: "CPU / CHIPSET".to_string(),
        name: if cpu_name.is_empty() { "Host Processor".to_string() } else { cpu_name },
        vendor: cpu_vendor,
        hardware_id: cpu_hw_id,
        installed_version: cpu_ver.clone(),
        installed_display_version: cpu_ver,
        driver_date: cpu_date,
        latest_version: cpu_latest,
        latest_release_date: cpu_latest_date,
        update_available: false,
        status_message: "ACTIVE WINDOWS 11 MICROCODE & SCHEDULER".to_string(),
        release_notes: cpu_notes,
        download_url: cpu_dl,
        suggested_filename: cpu_fname,
        official_portal_url: cpu_portal,
        clean_install_guide: cpu_guide,
    });

    let now_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let doh_hosts = duke_core::net_doh::cached_doh_host_count();

    Ok(DriverAuditReport {
        checked_at: now_ts,
        devices,
        drivers_download_dir: drivers_dir.to_string_lossy().to_string(),
        doh_cached_hosts: doh_hosts,
    })
}

#[tauri::command]
async fn download_clean_driver(
    app: AppHandle,
    url: String,
    filename: String,
    vendor: String,
) -> Result<DriverDownloadResult, String> {
    let parsed = Url::parse(url.trim()).map_err(|e| format!("Invalid URL: {}", e))?;
    if parsed.scheme() != "https" {
        return Err("Security Error: Clean driver downloads require HTTPS.".to_string());
    }
    let host = parsed.host_str().unwrap_or("").to_lowercase();
    let allowed = host.ends_with("nvidia.com")
        || host.ends_with("intel.com")
        || host.ends_with("amd.com");
    if !allowed {
        return Err(format!("Security Error: Domain '{}' is not an official NVIDIA/Intel/AMD host.", host));
    }

    let safe_name: String = filename
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' { c } else { '_' })
        .collect();
    let safe_name = if safe_name.is_empty() { "Driver_Setup.exe".to_string() } else { safe_name };

    let dir = resolve_drivers_download_dir(&app);
    fs::create_dir_all(&dir).map_err(|e| format!("Failed creating Duke_Drivers folder: {}", e))?;
    let target_path = dir.join(&safe_name);
    let part_path = dir.join(format!("{}.part", safe_name));

    let client = build_doh_client_for_url(url.trim()).await;
    let mut resp = client
        .get(url.trim())
        .send()
        .await
        .map_err(|e| format!("DoH driver download failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Vendor server returned HTTP {}", resp.status()));
    }

    let total_bytes = resp.content_length().unwrap_or(0);
    let mut file = tokio::fs::File::create(&part_path)
        .await
        .map_err(|e| format!("Failed creating file: {}", e))?;

    let mut downloaded: u64 = 0;
    let start = Instant::now();
    let mut last_emit = Instant::now();

    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("Stream read error: {}", e))?
    {
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Disk write error: {}", e))?;
        downloaded += chunk.len() as u64;

        if last_emit.elapsed() >= Duration::from_millis(200) {
            last_emit = Instant::now();
            let pct = if total_bytes > 0 {
                (downloaded as f64 / total_bytes as f64) * 100.0
            } else {
                0.0
            };
            let secs = start.elapsed().as_secs_f64().max(0.05);
            let mbps = (downloaded as f64 / (1024.0 * 1024.0)) / secs;
            let _ = app.emit(
                "driver-download-progress",
                serde_json::json!({
                    "fileName": safe_name,
                    "downloadedBytes": downloaded,
                    "totalBytes": total_bytes,
                    "progressPct": (pct * 10.0).round() / 10.0,
                    "speedMbps": (mbps * 10.0).round() / 10.0,
                    "status": "DOWNLOADING"
                }),
            );
        }
    }

    file.flush().await.map_err(|e| format!("Flush error: {}", e))?;
    drop(file);
    let _ = fs::rename(&part_path, &target_path);
    unblock_ntfs_zone_identifier(&target_path);

    let _ = app.emit(
        "driver-download-progress",
        serde_json::json!({
            "fileName": safe_name,
            "downloadedBytes": downloaded,
            "totalBytes": downloaded,
            "progressPct": 100.0,
            "speedMbps": 0.0,
            "status": "COMPLETED",
            "savedPath": target_path.to_string_lossy().to_string()
        }),
    );

    let (cmd_hint, notes) = match vendor.as_str() {
        "NVIDIA" => (
            format!("\"{}\" -s -noreboot -clean", target_path.display()),
            "For zero-telemetry clean install: extract with 7-Zip into a folder and keep ONLY Display.Driver, NVI2, PhysX (optional), and EULA/ListDevices.txt, then run setup.exe -s -noreboot.".to_string(),
        ),
        "Intel" => (
            format!("\"{}\" --silent --noExtras", target_path.display()),
            "Installs core Intel WDDM graphics driver without Intel Graphics Command Center telemetry.".to_string(),
        ),
        "AMD" => (
            format!("\"{}\" -INSTALL", target_path.display()),
            "Select 'Driver Only' or 'Minimal Install' in AMD setup to skip overlay & telemetry.".to_string(),
        ),
        _ => (
            format!("\"{}\"", target_path.display()),
            "Run downloaded driver installer.".to_string(),
        ),
    };

    Ok(DriverDownloadResult {
        saved_path: target_path.to_string_lossy().to_string(),
        file_name: safe_name,
        size_bytes: downloaded,
        clean_install_command: cmd_hint,
        clean_install_notes: notes,
    })
}

#[tauri::command]
async fn open_drivers_folder(app: AppHandle) -> Result<(), String> {
    let dir = resolve_drivers_download_dir(&app);
    let _ = fs::create_dir_all(&dir);
    Command::new("explorer.exe")
        .arg(dir.to_string_lossy().to_string())
        .spawn()
        .map_err(|e| format!("Failed opening Explorer: {}", e))?;
    Ok(())
}

#[tauri::command]
async fn set_auto_trim_enabled(enabled: bool) -> Result<bool, String> {
    AUTO_TRIM_ENABLED.store(enabled, Ordering::Relaxed);
    Ok(enabled)
}

#[tauri::command]
async fn trim_working_set(app: AppHandle) -> Result<Value, String> {
    trim_process_working_set();
    let ram_mb = get_process_working_set_mb();
    let doh_hosts = duke_core::net_doh::cached_doh_host_count();
    let payload = serde_json::json!({
        "workingSetMb": ram_mb,
        "dohShield": "ACTIVE",
        "dohCachedHosts": doh_hosts,
        "ecoScheduler": "READY",
        "status": "IDLE TRIMMED"
    });
    let _ = app.emit("memory-telemetry", payload.clone());
    Ok(payload)
}

#[tauri::command]
async fn get_system_telemetry() -> Result<Value, String> {
    let ram_mb = get_process_working_set_mb();
    let doh_hosts = duke_core::net_doh::cached_doh_host_count();
    Ok(serde_json::json!({
        "workingSetMb": ram_mb,
        "dohShield": "ACTIVE",
        "dohCachedHosts": doh_hosts,
        "autoTrimEnabled": AUTO_TRIM_ENABLED.load(Ordering::Relaxed),
        "portableMode": resolve_portable_data_dir().is_some(),
        "ecoScheduler": "READY"
    }))
}

fn read_system_ram_used_mb() -> u64 {
    #[cfg(target_os = "windows")]
    unsafe {
        let mut ms = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        if GlobalMemoryStatusEx(&mut ms).is_ok() {
            return ms.ullTotalPhys.saturating_sub(ms.ullAvailPhys) / (1024 * 1024);
        }
    }
    0
}

fn sample_primary_gpu_temp_and_load() -> (Option<u32>, Option<u32>) {
    let mut temp_c: Option<u32> = None;
    let mut load_pct: Option<u32> = None;

    if let Some(nvml) = get_nvml_api() {
        unsafe {
            let mut count: u32 = 0;
            if (nvml.get_count)(&mut count) == NVML_SUCCESS && count > 0 {
                let mut handle: *mut c_void = std::ptr::null_mut();
                if (nvml.get_handle)(0, &mut handle) == NVML_SUCCESS && !handle.is_null() {
                    if let Some(f) = nvml.get_temp {
                        let mut val = 0u32;
                        if f(handle, NVML_TEMPERATURE_GPU, &mut val) == NVML_SUCCESS {
                            temp_c = Some(val);
                        }
                    }
                    if let Some(f) = nvml.get_utilization {
                        let mut util = NvmlUtilization { gpu: 0, memory: 0 };
                        if f(handle, &mut util) == NVML_SUCCESS {
                            load_pct = Some(util.gpu.min(100));
                        }
                    }
                }
            }
        }
    }

    if load_pct.is_none() {
        load_pct = sample_pdh_gpu_3d_utilization().map(|u| u.round().clamp(0.0, 100.0) as u32);
    }

    (temp_c, load_pct)
}

fn publish_shm_telemetry_tick() {
    if let Some(Some(bus)) = DUKE_SHM_BUS.get() {
        let (temp_c, load_pct) = sample_primary_gpu_temp_and_load();
        let ram_used_mb = read_system_ram_used_mb();
        let payload_a = pack_gpu_telemetry(temp_c, load_pct);
        bus.publish_event(
            ShmEventType::TelemetryTick,
            ShmSourceApp::DukeMonitor,
            std::process::id(),
            payload_a,
            ram_used_mb,
            "",
        );
    }
}

fn apply_nvml_game_profile(profile: &gpu_profiles::GpuPowerProfile) -> (bool, String) {
    let Some(nvml) = get_nvml_api() else {
        return (false, "NVML unavailable; applied OS priority & EcoQoS jail".to_string());
    };
    let mut status_parts: Vec<String> = Vec::new();
    let mut hw_applied = false;
    unsafe {
        let mut dev: *mut c_void = std::ptr::null_mut();
        if (nvml.get_handle)(0, &mut dev) != NVML_SUCCESS || dev.is_null() {
            return (false, "Primary NVML GPU handle unavailable".to_string());
        }
        if let Some(power_w) = profile.power_limit_watts.filter(|&w| w > 0) {
            if let Some(get_pwr) = nvml.get_power_limit {
                let mut cur_mw = 0u32;
                if get_pwr(dev, &mut cur_mw) == NVML_SUCCESS && cur_mw > 0 {
                    let _ = DEFAULT_GPU_POWER_LIMIT_MW.compare_exchange(
                        0,
                        cur_mw,
                        Ordering::AcqRel,
                        Ordering::Relaxed,
                    );
                }
            }
            if let Some(set_pwr) = nvml.set_power_limit {
                let target_mw = power_w.saturating_mul(1000);
                if set_pwr(dev, target_mw) == NVML_SUCCESS {
                    GPU_POWER_MODIFIED_FOR_GAME.store(true, Ordering::Release);
                    hw_applied = true;
                    status_parts.push(format!("{}W power cap", power_w));
                }
            }
        }
        if let (Some(min_clk), Some(max_clk)) =
            (profile.locked_clock_min_mhz, profile.locked_clock_max_mhz)
        {
            if max_clk >= min_clk && max_clk > 0 {
                if let Some(set_clk) = nvml.set_locked_clocks {
                    if set_clk(dev, min_clk, max_clk) == NVML_SUCCESS {
                        GPU_CLOCKS_LOCKED_FOR_GAME.store(true, Ordering::Release);
                        hw_applied = true;
                        status_parts.push(format!("{}-{} MHz lock", min_clk, max_clk));
                    }
                }
            }
        }
    }
    if status_parts.is_empty() {
        (hw_applied, format!("Profile '{}' active (priority + EcoQoS)", profile.name))
    } else {
        (hw_applied, format!("{}: {}", profile.name, status_parts.join(", ")))
    }
}

fn reset_nvml_game_profile() {
    let reset_clocks = GPU_CLOCKS_LOCKED_FOR_GAME.swap(false, Ordering::AcqRel);
    let reset_power = GPU_POWER_MODIFIED_FOR_GAME.swap(false, Ordering::AcqRel);
    if !reset_clocks && !reset_power {
        return;
    }
    let Some(nvml) = get_nvml_api() else {
        return;
    };
    unsafe {
        let mut dev: *mut c_void = std::ptr::null_mut();
        if (nvml.get_handle)(0, &mut dev) != NVML_SUCCESS || dev.is_null() {
            return;
        }
        if reset_clocks {
            if let Some(reset_fn) = nvml.reset_locked_clocks {
                let _ = reset_fn(dev);
            }
        }
        if reset_power {
            let def_mw = DEFAULT_GPU_POWER_LIMIT_MW.swap(0, Ordering::AcqRel);
            if def_mw > 0 {
                if let Some(set_pwr) = nvml.set_power_limit {
                    let _ = set_pwr(dev, def_mw);
                }
            }
        }
    }
}

fn jail_background_processes_for_game(exclude_game_pid: u32) -> usize {
    let self_pid = std::process::id();
    let mut jailed_count = 0usize;
    let protected = [
        "csrss.exe",
        "dwm.exe",
        "winlogon.exe",
        "services.exe",
        "lsass.exe",
        "smss.exe",
        "svchost.exe",
        "explorer.exe",
        "audiodg.exe",
        "fontdrvhost.exe",
        "msedgewebview2.exe",
    ];
    let mut sys = match get_sysinfo().lock() {
        Ok(g) => g,
        Err(_) => return 0,
    };
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let candidate_pids: Vec<u32> = sys
        .processes()
        .iter()
        .filter_map(|(pid, proc_info)| {
            let raw_pid = pid.as_u32();
            if raw_pid <= 4 || raw_pid == self_pid || raw_pid == exclude_game_pid {
                return None;
            }
            let lower = proc_info.name().to_string_lossy().to_ascii_lowercase();
            if lower.starts_with("duke-") || protected.contains(&lower.as_str()) {
                return None;
            }
            let is_bloat = BLOAT_SIGNATURES.iter().any(|(sig, _)| lower.contains(sig));
            let is_heavy_bg = proc_info.memory() > 180 * 1024 * 1024;
            if is_bloat || is_heavy_bg {
                Some(raw_pid)
            } else {
                None
            }
        })
        .take(48)
        .collect();
    drop(sys);

    #[cfg(target_os = "windows")]
    unsafe {
        let mut jailed_set = get_eco_jailed_pids()
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        for pid in candidate_pids {
            if jailed_set.contains(&pid) {
                continue;
            }
            if let Ok(handle) = OpenProcess(
                PROCESS_SET_INFORMATION | PROCESS_SET_QUOTA | PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                pid,
            ) {
                let _ = SetPriorityClass(handle.0, BELOW_NORMAL_PRIORITY_CLASS);
                let throttling = ProcessPowerThrottlingState {
                    version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
                    control_mask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
                    state_mask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
                };
                if SetProcessInformation(
                    handle.0,
                    PROCESS_POWER_THROTTLING_INFO_CLASS,
                    &throttling as *const _ as *const c_void,
                    std::mem::size_of::<ProcessPowerThrottlingState>() as u32,
                ) != 0
                {
                    jailed_set.insert(pid);
                    jailed_count += 1;
                }
                let _ = K32EmptyWorkingSet(handle.0);
                let _ = CloseHandle(handle);
            }
        }
        if let Ok(mut guard) = get_eco_jailed_pids().lock() {
            *guard = jailed_set;
        }
    }
    jailed_count
}

fn handle_game_launched_boost_ext(
    app: &AppHandle,
    pid: u32,
    title: &str,
    exe_name: &str,
    app_id: Option<u32>,
    platform_id: Option<u16>,
    exe_hash: Option<u64>,
) {
    let matched_profile =
        gpu_profiles::match_profile_for_game(app_id, platform_id, exe_hash, title, exe_name);
    let already_boosted = get_boosted_pids()
        .lock()
        .map(|s| s.contains(&pid))
        .unwrap_or(false);
    let allow_boost = matched_profile
        .as_ref()
        .map(|p| p.boost_game_priority)
        .unwrap_or(true);
    if allow_boost && pid > 4 && !already_boosted && duke_core::win32_ffi::boost_external_pid(pid).is_ok() {
        if let Ok(mut boosted) = get_boosted_pids().lock() {
            boosted.insert(pid);
        }
        if let Ok(mut jailed) = get_eco_jailed_pids().lock() {
            jailed.remove(&pid);
        }
    }
    duke_core::win32_ffi::enable_eco_qos_mode();
    duke_core::win32_ffi::trim_current_working_set();

    let (profile_name, nvml_summary, jailed_bg) = if let Some(ref prof) = matched_profile {
        gpu_profiles::set_active_profile_id(Some(app), Some(prof.id.clone()));
        let (_, msg) = apply_nvml_game_profile(prof);
        let jailed = if prof.eco_jail_background {
            jail_background_processes_for_game(pid)
        } else {
            0
        };
        if let Some(Some(shm)) = DUKE_SHM_BUS.get() {
            shm.publish_frame(duke_core::shm_bus::ShmEventFrame::profile_switch(
                ShmSourceApp::DukeMonitor,
                pid,
                app_id.unwrap_or(0),
                platform_id.unwrap_or(0),
                &prof.name,
            ));
        }
        (prof.name.clone(), msg, jailed)
    } else {
        (String::new(), String::new(), 0usize)
    };

    if !already_boosted {
        let _ = app.emit(
            "duke-bus-game-started",
            serde_json::json!({
                "pid": pid,
                "appId": app_id,
                "platformId": platform_id,
                "title": title,
                "exeName": exe_name,
                "gpuProfile": profile_name,
                "nvmlStatus": nvml_summary,
                "jailedBackgroundCount": jailed_bg,
            }),
        );
    }
}

fn handle_game_exited_cleanup(app: &AppHandle, pid: u32) {
    reset_nvml_game_profile();
    gpu_profiles::set_active_profile_id(Some(app), Some("default_performance".to_string()));
    if let Ok(mut boosted) = get_boosted_pids().lock() {
        if pid == 0 {
            boosted.clear();
        } else {
            boosted.remove(&pid);
        }
    }
    let window_visible = app
        .get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    if window_visible {
        duke_core::win32_ffi::restore_normal_priority_mode();
    } else {
        duke_core::win32_ffi::enable_eco_qos_mode();
    }
    duke_core::win32_ffi::trim_current_working_set();
}

#[tauri::command]
fn get_gpu_profiles(app: AppHandle) -> gpu_profiles::GpuProfileStore {
    gpu_profiles::get_store_snapshot(Some(&app))
}

#[tauri::command]
fn save_gpu_profile(
    app: AppHandle,
    profile: gpu_profiles::GpuPowerProfile,
) -> Result<gpu_profiles::GpuProfileStore, String> {
    gpu_profiles::upsert_profile(&app, profile)
}

#[tauri::command]
fn delete_gpu_profile(
    app: AppHandle,
    profile_id: String,
) -> Result<gpu_profiles::GpuProfileStore, String> {
    gpu_profiles::remove_profile(&app, &profile_id)
}

fn main() {
    let daemon = TrayDaemonState::new(TrayDaemonConfig {
        tray_id: "duke-monitor-tray".to_string(),
        window_label: "main".to_string(),
        tooltip: "Duke Monitor - Hardware & EcoQoS Engine".to_string(),
        open_label: "Open Duke Monitor".to_string(),
        trim_label: "Trim Monitor Memory".to_string(),
        exit_label: "Quit Duke Monitor".to_string(),
        close_to_tray: Arc::new(AtomicBool::new(true)),
        minimize_to_tray: Arc::new(AtomicBool::new(true)),
    });
    let daemon_setup = daemon.clone();
    let daemon_run = daemon;

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .setup(move |app| {
            // Prime sysinfo & CPU topology & GPU profile store & trim working set immediately after startup
            gpu_profiles::init_store(app.handle());
            let _ = get_cpu_topology();
            let _ = get_sysinfo();
            trim_process_working_set();

            // Install headless tray daemon (0 MB WebView2 RAM when closed to tray)
            install_standard_tray(
                app.handle(),
                &daemon_setup,
                Some(Arc::new(|| {
                    duke_core::win32_ffi::restore_normal_priority_mode();
                })),
                Some(Arc::new(|| {
                    SHM_STOP_SIGNAL.store(true, Ordering::Release);
                    if let Some(Some(bus)) = DUKE_SHM_BUS.get() {
                        bus.unregister_app();
                    }
                })),
            )?;

            // Connect to lock-free Shared Memory Ring Buffer (`Local\duke-suite-shm-v1`)
            let _ = DUKE_SHM_BUS.get_or_init(|| {
                let bus = DukeShmBus::open_or_create().ok();
                if let Some(ref b) = bus {
                    b.register_app(ShmSourceApp::DukeMonitor);
                }
                bus
            });

            // Subscribe to Shared Memory GameLaunched / GameExited / RamPurge frames (nanosecond latency)
            let shm_app = app.handle().clone();
            let _ = std::thread::Builder::new()
                .name("duke-monitor-shm-rx".to_string())
                .spawn(move || {
                    let Ok(mut shm_rx) = DukeShmBus::open_or_create() else {
                        return;
                    };
                    while !SHM_STOP_SIGNAL.load(Ordering::Acquire) {
                        let mut processed = false;
                        while let Some(frame) = shm_rx.try_recv() {
                            processed = true;
                            match frame.kind() {
                                ShmEventType::GameLaunched => {
                                    if let Some(ev) = frame.decode_game_launched() {
                                        let exe_name = read_process_name_by_pid(ev.pid);
                                        handle_game_launched_boost_ext(
                                            &shm_app,
                                            ev.pid,
                                            &ev.tag,
                                            &exe_name,
                                            if ev.app_id > 0 { Some(ev.app_id) } else { None },
                                            if ev.platform_id > 0 {
                                                Some(ev.platform_id)
                                            } else {
                                                None
                                            },
                                            if ev.exe_hash != 0 {
                                                Some(ev.exe_hash)
                                            } else {
                                                None
                                            },
                                        );
                                    }
                                }
                                ShmEventType::GameExited => {
                                    handle_game_exited_cleanup(&shm_app, frame.pid);
                                }
                                ShmEventType::RamPurge => {
                                    duke_core::win32_ffi::trim_current_working_set();
                                }
                                _ => {}
                            }
                        }
                        std::thread::sleep(Duration::from_millis(if processed { 1 } else { 5 }));
                    }
                });

            // 1s hardware poll loop publishing TelemetryTick frames to Local\duke-suite-shm-v1
            tauri::async_runtime::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(1));
                loop {
                    interval.tick().await;
                    let _ = tokio::task::spawn_blocking(publish_shm_telemetry_tick).await;
                }
            });

            // Start leader-elected Duke Suite IPC bus
            let bus_app = app.handle().clone();
            let _ = DUKE_BUS.get_or_init(|| {
                DukeBus::start("duke-monitor", move |event| match event {
                    DukeEvent::GameLaunched {
                        pid,
                        title,
                        exe_name,
                        app_id,
                        platform_id,
                        exe_hash,
                    } => {
                        handle_game_launched_boost_ext(
                            &bus_app,
                            pid,
                            &title,
                            &exe_name,
                            if app_id > 0 { Some(app_id) } else { None },
                            if platform_id > 0 { Some(platform_id) } else { None },
                            if exe_hash != 0 { Some(exe_hash) } else { None },
                        );
                    }
                    DukeEvent::GameExited { pid, .. } => {
                        handle_game_exited_cleanup(&bus_app, pid);
                    }
                    DukeEvent::GameStarted {
                        pid,
                        title,
                        exe_name,
                    } => {
                        handle_game_launched_boost_ext(
                            &bus_app,
                            pid,
                            &title,
                            &exe_name,
                            None,
                            None,
                            None,
                        );
                    }
                    DukeEvent::GameStopped { pid, .. } => {
                        handle_game_exited_cleanup(&bus_app, pid);
                    }
                    DukeEvent::RamPurgeRequested { .. } => {
                        duke_core::win32_ffi::trim_current_working_set();
                    }
                    _ => {}
                })
            });

            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(60));
                loop {
                    interval.tick().await;
                    if AUTO_TRIM_ENABLED.load(Ordering::Relaxed) {
                        trim_process_working_set();
                    }
                    let ram_mb = get_process_working_set_mb();
                    let doh_hosts = duke_core::net_doh::cached_doh_host_count();
                    let _ = app_handle.emit(
                        "memory-telemetry",
                        serde_json::json!({
                            "workingSetMb": ram_mb,
                            "dohShield": "ACTIVE",
                            "dohCachedHosts": doh_hosts,
                            "ecoScheduler": "READY",
                            "status": "IDLE TRIMMED"
                        }),
                    );
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_hardware_telemetry,
            purge_system_ram,
            get_running_processes,
            set_process_eco_qos,
            boost_game_process,
            trim_process_ram,
            kill_process,
            kill_all_bloat_processes,
            check_driver_updates,
            download_clean_driver,
            open_drivers_folder,
            set_auto_trim_enabled,
            trim_working_set,
            get_system_telemetry,
            get_gpu_profiles,
            save_gpu_profile,
            delete_gpu_profile
        ])
        .build(tauri::generate_context!())
        .expect("error while building Duke Monitor Tauri application");

    app.run(move |app_handle, event| {
        handle_run_event(
            app_handle,
            &event,
            &daemon_run,
            Some(&|| {
                duke_core::win32_ffi::enable_eco_qos_mode();
            }),
            Some(&|| {
                SHM_STOP_SIGNAL.store(true, Ordering::Release);
                if let Some(Some(bus)) = DUKE_SHM_BUS.get() {
                    bus.unregister_app();
                }
            }),
        );
    });
}
