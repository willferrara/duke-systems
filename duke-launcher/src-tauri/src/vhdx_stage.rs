use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

use duke_core::nvme_prefetch::{ioring_copy_file, probe_ioring_support};

use crate::snapshots::CAS_DIR_NAME;
use crate::{
    atomic_write_string, format_bytes, get_app_data_file_standalone, load_library_standalone,
    resolve_game_root_dir,
};

const VHDX_STATE_FILE: &str = "vhdx_stage_state.json";
const HOT_TIER_SUBDIR: &str = "HotTier";
const VHDX_CONTAINER_EXT: &str = "vhdx";
const DEFAULT_HOT_TIER_BUDGET_MB: u64 = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtDiskRuntimeCapability {
    pub virtdisk_dll_loaded: bool,
    pub create_virtual_disk: bool,
    pub open_virtual_disk: bool,
    pub attach_virtual_disk: bool,
    pub detach_virtual_disk: bool,
    pub ioring_available: bool,
    pub windows_build: u32,
    pub hot_tier_root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameVhdxEntryState {
    pub game_id: String,
    pub game_root: String,
    pub source_volume: String,
    pub source_on_hdd: bool,
    pub total_files: usize,
    pub small_files_under_1mb: usize,
    pub total_bytes: u64,
    pub vhdx_path: Option<String>,
    pub vhdx_file_bytes: u64,
    pub packed_archive_path: Option<String>,
    pub hot_stage_dir: Option<String>,
    pub hot_staged: bool,
    pub hot_staged_bytes: u64,
    pub hot_staged_files: usize,
    pub auto_stage_on_launch: bool,
    pub attach_mode: String,
    pub ioring_used: bool,
    pub updated_at_epoch_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct VhdxStageStore {
    games: HashMap<String, GameVhdxEntryState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VhdxContainerizeReport {
    pub game_id: String,
    pub vhdx_path: String,
    pub packed_archive_path: String,
    pub vhdx_virtual_bytes: u64,
    pub vhdx_physical_bytes: u64,
    pub packed_bytes: u64,
    pub small_files_consolidated: usize,
    pub ioring_used: bool,
    pub attach_mode: String,
    pub elapsed_ms: u64,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotStageReport {
    pub game_id: String,
    pub staged_dir: String,
    pub files_transferred: usize,
    pub bytes_transferred: u64,
    pub bytes_formatted: String,
    pub used_ioring: bool,
    pub attach_attempted: bool,
    pub attached_physical_path: Option<String>,
    pub elapsed_ms: u64,
    pub auto_stage_on_launch: bool,
    pub status_message: String,
}

#[cfg(windows)]
mod win_virtdisk {
    use std::ffi::c_void;
    use std::path::Path;
    use std::sync::OnceLock;
    use windows::core::{GUID, PCSTR, PCWSTR};
    use windows::Win32::Foundation::{CloseHandle, HANDLE, HMODULE, INVALID_HANDLE_VALUE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::IO::DeviceIoControl;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    const VIRTUAL_STORAGE_TYPE_DEVICE_VHDX: u32 = 3;
    const CREATE_VIRTUAL_DISK_VERSION_2: i32 = 2;
    const OPEN_VIRTUAL_DISK_VERSION_2: i32 = 2;
    const ATTACH_VIRTUAL_DISK_VERSION_1: i32 = 1;
    const CREATE_VIRTUAL_DISK_FLAG_NONE: u32 = 0;
    const OPEN_VIRTUAL_DISK_FLAG_NONE: u32 = 0;
    const VIRTUAL_DISK_ACCESS_NONE: u32 = 0;
    const VIRTUAL_DISK_ACCESS_ALL: u32 = 0x003f_0000;
    const ATTACH_VIRTUAL_DISK_FLAG_NO_LOCAL_HOST: u32 = 0x0000_0004;
    const DETACH_VIRTUAL_DISK_FLAG_NONE: u32 = 0;

    fn ms_vendor_guid() -> GUID {
        GUID::from_values(
            0xec98_4aec,
            0xa0f9,
            0x47e9,
            [0x90, 0x1f, 0x71, 0x41, 0x5a, 0x66, 0x34, 0x5b],
        )
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct VirtualStorageType {
        device_id: u32,
        vendor_id: GUID,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CreateVirtualDiskVersion2 {
        unique_id: GUID,
        maximum_size: u64,
        block_size_in_bytes: u32,
        sector_size_in_bytes: u32,
        parent_path: *const u16,
        source_path: *const u16,
        open_flags: u32,
        parent_virtual_storage_type: VirtualStorageType,
        resiliency_guid: GUID,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CreateVirtualDiskParametersV2 {
        version: i32,
        _pad: u32,
        v2: CreateVirtualDiskVersion2,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct OpenVirtualDiskParametersV2 {
        version: i32,
        get_info_only: i32,
        read_only: i32,
        resiliency_guid: GUID,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct AttachVirtualDiskParametersV1 {
        version: i32,
        reserved: u32,
    }

    type CreateVirtualDiskFn = unsafe extern "system" fn(
        *const VirtualStorageType,
        PCWSTR,
        u32,
        *const c_void,
        u32,
        u32,
        *const CreateVirtualDiskParametersV2,
        *mut c_void,
        *mut HANDLE,
    ) -> u32;

    type OpenVirtualDiskFn = unsafe extern "system" fn(
        *const VirtualStorageType,
        PCWSTR,
        u32,
        u32,
        *const OpenVirtualDiskParametersV2,
        *mut HANDLE,
    ) -> u32;

    type AttachVirtualDiskFn = unsafe extern "system" fn(
        HANDLE,
        *const c_void,
        u32,
        u32,
        *const AttachVirtualDiskParametersV1,
        *mut c_void,
    ) -> u32;

    type DetachVirtualDiskFn = unsafe extern "system" fn(HANDLE, u32, u32) -> u32;

    type GetVirtualDiskPhysicalPathFn = unsafe extern "system" fn(HANDLE, *mut u32, *mut u16) -> u32;

    pub struct VirtDiskProcs {
        pub loaded: bool,
        pub has_create: bool,
        pub has_open: bool,
        pub has_attach: bool,
        pub has_detach: bool,
        create_fn: Option<CreateVirtualDiskFn>,
        open_fn: Option<OpenVirtualDiskFn>,
        attach_fn: Option<AttachVirtualDiskFn>,
        detach_fn: Option<DetachVirtualDiskFn>,
        physical_path_fn: Option<GetVirtualDiskPhysicalPathFn>,
    }

    // SAFETY: Exported VirtDisk function pointers from VirtDisk.dll stay pinned in process address space.
    unsafe impl Send for VirtDiskProcs {}
    unsafe impl Sync for VirtDiskProcs {}

    static VIRTDISK_PROCS: OnceLock<VirtDiskProcs> = OnceLock::new();

    fn wide_null(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn get_virtdisk_procs() -> &'static VirtDiskProcs {
        VIRTDISK_PROCS.get_or_init(|| {
            let dll = wide_null("VirtDisk.dll");
            let hmod: HMODULE = match unsafe { LoadLibraryW(PCWSTR(dll.as_ptr())) } {
                Ok(h) if !h.is_invalid() => h,
                _ => {
                    return VirtDiskProcs {
                        loaded: false,
                        has_create: false,
                        has_open: false,
                        has_attach: false,
                        has_detach: false,
                        create_fn: None,
                        open_fn: None,
                        attach_fn: None,
                        detach_fn: None,
                        physical_path_fn: None,
                    };
                }
            };
            unsafe {
                let create_fn =
                    GetProcAddress(hmod, PCSTR(c"CreateVirtualDisk".as_ptr().cast()))
                        .map(|p| std::mem::transmute::<_, CreateVirtualDiskFn>(p));
                let open_fn =
                    GetProcAddress(hmod, PCSTR(c"OpenVirtualDisk".as_ptr().cast()))
                        .map(|p| std::mem::transmute::<_, OpenVirtualDiskFn>(p));
                let attach_fn =
                    GetProcAddress(hmod, PCSTR(c"AttachVirtualDisk".as_ptr().cast()))
                        .map(|p| std::mem::transmute::<_, AttachVirtualDiskFn>(p));
                let detach_fn =
                    GetProcAddress(hmod, PCSTR(c"DetachVirtualDisk".as_ptr().cast()))
                        .map(|p| std::mem::transmute::<_, DetachVirtualDiskFn>(p));
                let physical_path_fn = GetProcAddress(
                    hmod,
                    PCSTR(c"GetVirtualDiskPhysicalPath".as_ptr().cast()),
                )
                .map(|p| std::mem::transmute::<_, GetVirtualDiskPhysicalPathFn>(p));

                VirtDiskProcs {
                    loaded: true,
                    has_create: create_fn.is_some(),
                    has_open: open_fn.is_some(),
                    has_attach: attach_fn.is_some(),
                    has_detach: detach_fn.is_some(),
                    create_fn,
                    open_fn,
                    attach_fn,
                    detach_fn,
                    physical_path_fn,
                }
            }
        })
    }

    pub fn create_vhdx_file(vhdx_path: &Path, max_size_bytes: u64) -> Result<(), String> {
        let procs = get_virtdisk_procs();
        let create_fn = procs
            .create_fn
            .ok_or_else(|| "VirtDisk.dll CreateVirtualDisk unavailable".to_string())?;

        if let Some(parent) = vhdx_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed creating VHDX folder {}: {}", parent.display(), e))?;
        }
        if vhdx_path.exists() {
            let _ = std::fs::remove_file(vhdx_path);
        }

        // VHDX minimum size is 3 MiB and must be aligned to 512 bytes / 1 MiB cleanly.
        let aligned_size = max_size_bytes.max(4 * 1024 * 1024).div_ceil(1024 * 1024) * 1024 * 1024;
        let storage_type = VirtualStorageType {
            device_id: VIRTUAL_STORAGE_TYPE_DEVICE_VHDX,
            vendor_id: ms_vendor_guid(),
        };
        let params = CreateVirtualDiskParametersV2 {
            version: CREATE_VIRTUAL_DISK_VERSION_2,
            _pad: 0,
            v2: CreateVirtualDiskVersion2 {
                unique_id: GUID::zeroed(),
                maximum_size: aligned_size,
                block_size_in_bytes: 0,   // OS default dynamic VHDX block size (32 MiB)
                sector_size_in_bytes: 0,  // OS default 4 KiB
                parent_path: std::ptr::null(),
                source_path: std::ptr::null(),
                open_flags: OPEN_VIRTUAL_DISK_FLAG_NONE,
                parent_virtual_storage_type: VirtualStorageType {
                    device_id: 0,
                    vendor_id: GUID::zeroed(),
                },
                resiliency_guid: GUID::zeroed(),
            },
        };

        let path_w = wide_null(&vhdx_path.to_string_lossy());
        let mut handle = HANDLE::default();
        let rc = unsafe {
            create_fn(
                &storage_type,
                PCWSTR(path_w.as_ptr()),
                VIRTUAL_DISK_ACCESS_NONE,
                std::ptr::null(),
                CREATE_VIRTUAL_DISK_FLAG_NONE,
                0,
                &params,
                std::ptr::null_mut(),
                &mut handle,
            )
        };
        if !handle.is_invalid() && !handle.0.is_null() {
            unsafe {
                let _ = CloseHandle(handle);
            }
        }
        if rc != 0 {
            return Err(format!("CreateVirtualDisk failed (Win32 error {})", rc));
        }
        Ok(())
    }

    pub fn try_attach_vhdx(vhdx_path: &Path) -> Result<Option<String>, String> {
        let procs = get_virtdisk_procs();
        let open_fn = procs
            .open_fn
            .ok_or_else(|| "VirtDisk.dll OpenVirtualDisk unavailable".to_string())?;
        let attach_fn = procs
            .attach_fn
            .ok_or_else(|| "VirtDisk.dll AttachVirtualDisk unavailable".to_string())?;

        let storage_type = VirtualStorageType {
            device_id: VIRTUAL_STORAGE_TYPE_DEVICE_VHDX,
            vendor_id: ms_vendor_guid(),
        };
        let open_params = OpenVirtualDiskParametersV2 {
            version: OPEN_VIRTUAL_DISK_VERSION_2,
            get_info_only: 0,
            read_only: 0,
            resiliency_guid: GUID::zeroed(),
        };
        let path_w = wide_null(&vhdx_path.to_string_lossy());
        let mut handle = HANDLE::default();
        let open_rc = unsafe {
            open_fn(
                &storage_type,
                PCWSTR(path_w.as_ptr()),
                VIRTUAL_DISK_ACCESS_NONE,
                OPEN_VIRTUAL_DISK_FLAG_NONE,
                &open_params,
                &mut handle,
            )
        };
        if open_rc != 0 || handle.is_invalid() {
            return Err(format!("OpenVirtualDisk failed (Win32 error {})", open_rc));
        }

        let attach_params = AttachVirtualDiskParametersV1 {
            version: ATTACH_VIRTUAL_DISK_VERSION_1,
            reserved: 0,
        };
        let attach_rc = unsafe {
            attach_fn(
                handle,
                std::ptr::null(),
                ATTACH_VIRTUAL_DISK_FLAG_NO_LOCAL_HOST,
                0,
                &attach_params,
                std::ptr::null_mut(),
            )
        };

        let mut phys_path = None;
        if attach_rc == 0 {
            if let Some(phys_fn) = procs.physical_path_fn {
                let mut buf = vec![0u16; 260];
                let mut sz = (buf.len() * 2) as u32;
                let prc = unsafe { phys_fn(handle, &mut sz, buf.as_mut_ptr()) };
                if prc == 0 {
                    let nul = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                    phys_path = Some(String::from_utf16_lossy(&buf[..nul]));
                }
            }
        }

        unsafe {
            let _ = CloseHandle(handle);
        }

        if attach_rc == 5 || attach_rc == 1314 {
            // Standard non-admin user: graceful fallback without breaking launch.
            return Ok(None);
        }
        if attach_rc != 0 {
            return Err(format!("AttachVirtualDisk failed (Win32 error {})", attach_rc));
        }
        Ok(phys_path)
    }

    pub fn try_detach_vhdx(vhdx_path: &Path) -> Result<bool, String> {
        let procs = get_virtdisk_procs();
        let open_fn = procs
            .open_fn
            .ok_or_else(|| "VirtDisk.dll OpenVirtualDisk unavailable".to_string())?;
        let detach_fn = procs
            .detach_fn
            .ok_or_else(|| "VirtDisk.dll DetachVirtualDisk unavailable".to_string())?;

        let storage_type = VirtualStorageType {
            device_id: VIRTUAL_STORAGE_TYPE_DEVICE_VHDX,
            vendor_id: ms_vendor_guid(),
        };
        let open_params = OpenVirtualDiskParametersV2 {
            version: OPEN_VIRTUAL_DISK_VERSION_2,
            get_info_only: 0,
            read_only: 0,
            resiliency_guid: GUID::zeroed(),
        };
        let path_w = wide_null(&vhdx_path.to_string_lossy());
        let mut handle = HANDLE::default();
        let open_rc = unsafe {
            open_fn(
                &storage_type,
                PCWSTR(path_w.as_ptr()),
                VIRTUAL_DISK_ACCESS_ALL,
                OPEN_VIRTUAL_DISK_FLAG_NONE,
                &open_params,
                &mut handle,
            )
        };
        if open_rc != 0 || handle.is_invalid() {
            let retry_rc = unsafe {
                open_fn(
                    &storage_type,
                    PCWSTR(path_w.as_ptr()),
                    VIRTUAL_DISK_ACCESS_NONE,
                    OPEN_VIRTUAL_DISK_FLAG_NONE,
                    &open_params,
                    &mut handle,
                )
            };
            if retry_rc != 0 || handle.is_invalid() {
                return Ok(false);
            }
        }
        let detach_rc = unsafe { detach_fn(handle, DETACH_VIRTUAL_DISK_FLAG_NONE, 0) };
        unsafe {
            let _ = CloseHandle(handle);
        }
        // ERROR_NOT_READY (21) / ERROR_DEV_NOT_EXIST (55) means already detached.
        Ok(detach_rc == 0 || detach_rc == 21 || detach_rc == 55)
    }

    pub fn query_drive_has_seek_penalty(path: &Path) -> bool {
        let s = path.to_string_lossy();
        let bytes = s.as_bytes();
        if bytes.len() < 2 || bytes[1] != b':' {
            return false;
        }
        let drive_letter = (bytes[0] as char).to_ascii_uppercase();
        let vol_path = wide_null(&format!("\\\\.\\{}:", drive_letter));
        let handle = match unsafe {
            CreateFileW(
                PCWSTR(vol_path.as_ptr()),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        } {
            Ok(h) if h != INVALID_HANDLE_VALUE => h,
            _ => return false,
        };

        #[repr(C)]
        struct StoragePropertyQuery {
            property_id: u32,
            query_type: u32,
            additional_parameters: [u8; 1],
        }
        #[repr(C)]
        struct DeviceSeekPenaltyDescriptor {
            version: u32,
            size: u32,
            incurs_seek_penalty: u8,
        }

        const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002d_1400;
        let query = StoragePropertyQuery {
            property_id: 7, // StorageDeviceSeekPenaltyProperty
            query_type: 0,  // PropertyStandardQuery
            additional_parameters: [0],
        };
        let mut out = DeviceSeekPenaltyDescriptor {
            version: 0,
            size: 0,
            incurs_seek_penalty: 0,
        };
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_STORAGE_QUERY_PROPERTY,
                Some(&query as *const _ as *const c_void),
                std::mem::size_of::<StoragePropertyQuery>() as u32,
                Some(&mut out as *mut _ as *mut c_void),
                std::mem::size_of::<DeviceSeekPenaltyDescriptor>() as u32,
                Some(&mut returned),
                None,
            )
        }
        .is_ok();

        unsafe {
            let _ = CloseHandle(handle);
        }
        ok && out.incurs_seek_penalty != 0
    }
}

#[cfg(not(windows))]
mod win_virtdisk {
    use std::path::Path;

    pub struct VirtDiskProcs {
        pub loaded: bool,
        pub create_fn: Option<()>,
        pub open_fn: Option<()>,
        pub attach_fn: Option<()>,
        pub detach_fn: Option<()>,
    }

    static STUB_PROCS: VirtDiskProcs = VirtDiskProcs {
        loaded: false,
        create_fn: None,
        open_fn: None,
        attach_fn: None,
        detach_fn: None,
    };

    pub fn get_virtdisk_procs() -> &'static VirtDiskProcs {
        &STUB_PROCS
    }

    pub fn create_vhdx_file(_vhdx_path: &Path, _max_size_bytes: u64) -> Result<(), String> {
        Err("VirtDisk.dll only available on Windows".to_string())
    }

    pub fn try_attach_vhdx(_vhdx_path: &Path) -> Result<Option<String>, String> {
        Ok(None)
    }

    pub fn try_detach_vhdx(_vhdx_path: &Path) -> Result<bool, String> {
        Ok(false)
    }

    pub fn query_drive_has_seek_penalty(_path: &Path) -> bool {
        false
    }
}

fn load_store() -> VhdxStageStore {
    let Ok(path) = get_app_data_file_standalone(VHDX_STATE_FILE) else {
        return VhdxStageStore::default();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_store(store: &VhdxStageStore) -> Result<(), String> {
    let path = get_app_data_file_standalone(VHDX_STATE_FILE)?;
    let json = serde_json::to_string_pretty(store)
        .map_err(|e| format!("Failed serializing VHDX state: {}", e))?;
    atomic_write_string(&path, &json)
}

pub fn hot_tier_root_dir() -> Result<PathBuf, String> {
    let sentinel = get_app_data_file_standalone(VHDX_STATE_FILE)?;
    let base = sentinel
        .parent()
        .ok_or_else(|| "Invalid app data root".to_string())?;
    Ok(base.join(HOT_TIER_SUBDIR))
}

fn sanitize_id_folder(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn volume_prefix_label(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let bytes = raw.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        format!("{}:\\", (bytes[0] as char).to_ascii_uppercase())
    } else {
        "UNC/Other".to_string()
    }
}

fn should_skip_vhdx_walk(entry: &walkdir::DirEntry, root: &Path) -> bool {
    if entry.path() == root {
        return false;
    }
    let name = entry.file_name().to_string_lossy();
    entry.file_type().is_dir()
        && (name.eq_ignore_ascii_case(CAS_DIR_NAME)
            || name.eq_ignore_ascii_case(".duke-ds")
            || name.eq_ignore_ascii_case(".duke-vhdx")
            || name.eq_ignore_ascii_case(".duke-profile")
            || name.eq_ignore_ascii_case("$RECYCLE.BIN")
            || name.eq_ignore_ascii_case("System Volume Information"))
}

fn measure_game_files(root: &Path) -> (usize, usize, u64, Vec<(PathBuf, String, u64)>) {
    let mut total_files = 0usize;
    let mut small_files = 0usize;
    let mut total_bytes = 0u64;
    let mut files = Vec::new();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !should_skip_vhdx_walk(e, root))
    {
        let entry = match entry {
            Ok(v) => v,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let rel = entry
            .path()
            .strip_prefix(root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        total_files += 1;
        if len < 1024 * 1024 {
            small_files += 1;
        }
        total_bytes = total_bytes.saturating_add(len);
        files.push((entry.path().to_path_buf(), rel, len));
    }

    (total_files, small_files, total_bytes, files)
}

fn pack_small_files_zstd_bundle(
    files: &[(PathBuf, String, u64)],
    bundle_path: &Path,
) -> Result<(usize, u64), String> {
    if let Some(parent) = bundle_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed creating VHDX bundle folder: {}", e))?;
    }
    let tmp = bundle_path.with_extension("pack.tmp");
    let file = File::create(&tmp)
        .map_err(|e| format!("Failed creating bundle {}: {}", tmp.display(), e))?;
    let writer = BufWriter::with_capacity(64 * 1024, file);
    let mut encoder = zstd::stream::write::Encoder::new(writer, 3)
        .map_err(|e| format!("Zstd encoder init failed: {}", e))?;

    encoder
        .write_all(b"DKVHDXPK")
        .map_err(|e| format!("Header write error: {}", e))?;

    let mut small_count = 0usize;
    let mut buf = vec![0u8; 64 * 1024];

    for (path, rel, len) in files {
        // Pack small cold assets (< 4 MiB) cleanly into a compressed single-file stream to avoid Defender/NTFS thrashing.
        if *len > 4 * 1024 * 1024 {
            continue;
        }
        let mut f = match File::open(path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let rel_bytes = rel.as_bytes();
        let rel_len = (rel_bytes.len() as u32).min(4096);
        encoder
            .write_all(&rel_len.to_le_bytes())
            .map_err(|e| format!("Bundle entry header error: {}", e))?;
        encoder
            .write_all(&rel_bytes[..rel_len as usize])
            .map_err(|e| format!("Bundle entry path error: {}", e))?;
        encoder
            .write_all(&len.to_le_bytes())
            .map_err(|e| format!("Bundle entry size error: {}", e))?;

        let mut remaining = *len;
        while remaining > 0 {
            let to_read = remaining.min(buf.len() as u64) as usize;
            let n = f
                .read(&mut buf[..to_read])
                .map_err(|e| format!("Read error packing {}: {}", path.display(), e))?;
            if n == 0 {
                break;
            }
            encoder
                .write_all(&buf[..n])
                .map_err(|e| format!("Bundle payload error: {}", e))?;
            remaining -= n as u64;
        }
        small_count += 1;
    }

    encoder
        .finish()
        .map_err(|e| format!("Zstd finish failed on VHDX bundle: {}", e))?;
    if bundle_path.exists() {
        let _ = fs::remove_file(bundle_path);
    }
    fs::rename(&tmp, bundle_path)
        .map_err(|e| format!("Failed finalizing VHDX bundle: {}", e))?;
    let packed_bytes = fs::metadata(bundle_path).map(|m| m.len()).unwrap_or(0);
    Ok((small_count, packed_bytes))
}

fn select_hot_files(
    mut files: Vec<(PathBuf, String, u64)>,
    budget_bytes: u64,
) -> Vec<(PathBuf, String, u64)> {
    // Prioritize executables, runtime DLLs, and large archive packs most sensitive to cold-drive seek latency.
    files.sort_by(|a, b| {
        let score = |rel: &str, len: u64| -> (u8, u64) {
            let lower = rel.to_ascii_lowercase();
            let tier = if lower.ends_with(".exe") || lower.ends_with(".dll") {
                3
            } else if lower.ends_with(".pak")
                || lower.ends_with(".ba2")
                || lower.ends_with(".forge")
                || lower.ends_with(".utoc")
                || lower.ends_with(".ucas")
                || lower.ends_with(".dkds")
            {
                2
            } else {
                1
            };
            (tier, len)
        };
        score(&b.1, b.2).cmp(&score(&a.1, a.2))
    });

    let mut chosen = Vec::new();
    let mut used = 0u64;
    for item in files {
        if used.saturating_add(item.2) > budget_bytes && !chosen.is_empty() {
            continue;
        }
        used = used.saturating_add(item.2);
        chosen.push(item);
        if chosen.len() >= 128 {
            break;
        }
    }
    chosen
}

fn probe_runtime_capability_internal() -> VirtDiskRuntimeCapability {
    let procs = win_virtdisk::get_virtdisk_procs();
    let ioring = probe_ioring_support();
    let hot_tier_root = hot_tier_root_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| String::from("HotTier"));
    VirtDiskRuntimeCapability {
        virtdisk_dll_loaded: procs.loaded,
        create_virtual_disk: procs.has_create,
        open_virtual_disk: procs.has_open,
        attach_virtual_disk: procs.has_attach,
        detach_virtual_disk: procs.has_detach,
        ioring_available: ioring.supported,
        windows_build: ioring.windows_build,
        hot_tier_root,
    }
}

fn inspect_game_state(game_id: &str, game_root: &Path) -> GameVhdxEntryState {
    let store = load_store();
    let existing = store.games.get(game_id).cloned();
    let (total_files, small_files, total_bytes, _) = measure_game_files(game_root);
    let source_volume = volume_prefix_label(game_root);
    let source_on_hdd = win_virtdisk::query_drive_has_seek_penalty(game_root);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    match existing {
        Some(mut prev) => {
            prev.game_root = game_root.to_string_lossy().to_string();
            prev.source_volume = source_volume;
            prev.source_on_hdd = source_on_hdd;
            prev.total_files = total_files;
            prev.small_files_under_1mb = small_files;
            prev.total_bytes = total_bytes;
            if let Some(ref vp) = prev.vhdx_path {
                prev.vhdx_file_bytes = fs::metadata(vp).map(|m| m.len()).unwrap_or(0);
            }
            prev.updated_at_epoch_sec = now;
            prev
        }
        None => GameVhdxEntryState {
            game_id: game_id.to_string(),
            game_root: game_root.to_string_lossy().to_string(),
            source_volume,
            source_on_hdd,
            total_files,
            small_files_under_1mb: small_files,
            total_bytes,
            vhdx_path: None,
            vhdx_file_bytes: 0,
            packed_archive_path: None,
            hot_stage_dir: None,
            hot_staged: false,
            hot_staged_bytes: 0,
            hot_staged_files: 0,
            auto_stage_on_launch: false,
            attach_mode: "Unstaged".to_string(),
            ioring_used: false,
            updated_at_epoch_sec: now,
        },
    }
}

pub fn ensure_game_hot_staged_before_launch(game_id: &str, game_root: &Path) -> Option<String> {
    let store = load_store();
    let entry = store.games.get(game_id)?;
    if !entry.auto_stage_on_launch {
        return None;
    }
    let stage_dir = match entry.hot_stage_dir.as_ref() {
        Some(d) => PathBuf::from(d),
        None => {
            let root = hot_tier_root_dir().ok()?;
            root.join(sanitize_id_folder(game_id))
        }
    };
    let (_, _, _, files) = measure_game_files(game_root);
    let budget = (DEFAULT_HOT_TIER_BUDGET_MB / 2).saturating_mul(1024 * 1024);
    let hot_files = select_hot_files(files, budget);
    let mut staged_count = 0usize;
    let mut used_ioring = false;

    for (src, rel, _) in hot_files {
        let dst = stage_dir.join(&rel);
        if dst.is_file() {
            let s_len = fs::metadata(&src).map(|m| m.len()).unwrap_or(0);
            let d_len = fs::metadata(&dst).map(|m| m.len()).unwrap_or(1);
            if s_len == d_len {
                continue;
            }
        }
        if let Ok(rep) = ioring_copy_file(&src, &dst) {
            staged_count += 1;
            used_ioring |= rep.used_ioring_api;
        }
    }
    if staged_count > 0 {
        Some(format!(
            "Hot-tiered {} hot blocks to NVMe (IoRing={})",
            staged_count, used_ioring
        ))
    } else {
        None
    }
}

#[tauri::command]
pub async fn get_vhdx_stage_status(
    game_id: Option<String>,
) -> Result<(VirtDiskRuntimeCapability, Option<GameVhdxEntryState>), String> {
    tokio::task::spawn_blocking(move || {
        let cap = probe_runtime_capability_internal();
        let entry = match game_id {
            Some(ref id) if !id.trim().is_empty() => {
                let lib = load_library_standalone()?;
                if let Some(game) = lib.iter().find(|g| g.id == id.trim()) {
                    let root = resolve_game_root_dir(Path::new(&game.exe_path));
                    if root.is_dir() {
                        Some(inspect_game_state(&game.id, &root))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        };
        Ok((cap, entry))
    })
    .await
    .map_err(|e| format!("VHDX stage status task failed: {}", e))?
}

#[tauri::command]
pub async fn containerize_game_to_vhdx(
    game_id: String,
    custom_container_dir: Option<String>,
) -> Result<VhdxContainerizeReport, String> {
    tokio::task::spawn_blocking(move || {
        let started = Instant::now();
        let lib = load_library_standalone()?;
        let game = lib
            .iter()
            .find(|g| g.id == game_id)
            .ok_or_else(|| format!("Game not found: {}", game_id))?;
        let game_root = resolve_game_root_dir(Path::new(&game.exe_path));
        if !game_root.is_dir() {
            return Err(format!(
                "Game install directory missing: {}",
                game_root.display()
            ));
        }

        let (total_files, _small, total_bytes, files) = measure_game_files(&game_root);
        if total_files == 0 {
            return Err("Game directory contains no containerizable files.".to_string());
        }

        let base_dir = match custom_container_dir {
            Some(ref d) if !d.trim().is_empty() => PathBuf::from(d.trim()),
            _ => game_root.join(".duke-vhdx"),
        };
        fs::create_dir_all(&base_dir)
            .map_err(|e| format!("Failed creating container directory: {}", e))?;

        let slug = sanitize_id_folder(&game.id);
        let vhdx_path = base_dir.join(format!("{}.{}", slug, VHDX_CONTAINER_EXT));
        let pack_path = base_dir.join(format!("{}.duke-vhdx-pack", slug));

        // 25% dynamic headroom + 64 MiB NTFS metadata margin.
        let virtual_bytes = total_bytes
            .saturating_add(total_bytes / 4)
            .saturating_add(64 * 1024 * 1024);
        win_virtdisk::create_vhdx_file(&vhdx_path, virtual_bytes)?;

        let (small_count, packed_bytes) = pack_small_files_zstd_bundle(&files, &pack_path)?;
        let vhdx_physical_bytes = fs::metadata(&vhdx_path).map(|m| m.len()).unwrap_or(0);

        // Stage the compact single-file pack through IoRing onto primary NVMe staging tier if available.
        let hot_root = hot_tier_root_dir()?;
        let hot_bundle = hot_root
            .join(&slug)
            .join(format!("{}.duke-vhdx-pack", slug));
        let ioring_rep = ioring_copy_file(&pack_path, &hot_bundle)?;

        let mut store = load_store();
        let mut entry = inspect_game_state(&game.id, &game_root);
        entry.vhdx_path = Some(vhdx_path.to_string_lossy().to_string());
        entry.vhdx_file_bytes = vhdx_physical_bytes;
        entry.packed_archive_path = Some(pack_path.to_string_lossy().to_string());
        entry.hot_stage_dir = Some(hot_root.join(&slug).to_string_lossy().to_string());
        entry.attach_mode = "VirtDisk VHDX + IoRing Packed Envelope".to_string();
        entry.ioring_used = ioring_rep.used_ioring_api;
        store.games.insert(game.id.clone(), entry);
        save_store(&store)?;

        Ok(VhdxContainerizeReport {
            game_id: game.id.clone(),
            vhdx_path: vhdx_path.to_string_lossy().to_string(),
            packed_archive_path: pack_path.to_string_lossy().to_string(),
            vhdx_virtual_bytes: virtual_bytes,
            vhdx_physical_bytes,
            packed_bytes,
            small_files_consolidated: small_count,
            ioring_used: ioring_rep.used_ioring_api,
            attach_mode: "VirtDisk VHDX Container + Zstd Small-File Consolidation".to_string(),
            elapsed_ms: started.elapsed().as_millis() as u64,
            notes: format!(
                "Created VirtDisk VHDX envelope & consolidated {} small assets into {} Zstd container.",
                small_count,
                format_bytes(packed_bytes)
            ),
        })
    })
    .await
    .map_err(|e| format!("VHDX containerize task failed: {}", e))?
}

#[tauri::command]
pub async fn stage_game_hot_tier(
    game_id: String,
    budget_mb: Option<u64>,
    auto_stage_on_launch: Option<bool>,
) -> Result<HotStageReport, String> {
    tokio::task::spawn_blocking(move || {
        let started = Instant::now();
        let lib = load_library_standalone()?;
        let game = lib
            .iter()
            .find(|g| g.id == game_id)
            .ok_or_else(|| format!("Game not found: {}", game_id))?;
        let game_root = resolve_game_root_dir(Path::new(&game.exe_path));
        if !game_root.is_dir() {
            return Err(format!(
                "Game install folder missing: {}",
                game_root.display()
            ));
        }

        let slug = sanitize_id_folder(&game.id);
        let stage_dir = hot_tier_root_dir()?.join(&slug);
        fs::create_dir_all(&stage_dir)
            .map_err(|e| format!("Failed creating NVMe hot-tier dir: {}", e))?;

        let (_, _, _, files) = measure_game_files(&game_root);
        let budget_bytes = budget_mb
            .unwrap_or(DEFAULT_HOT_TIER_BUDGET_MB)
            .clamp(128, 32768)
            .saturating_mul(1024 * 1024);
        let hot_files = select_hot_files(files, budget_bytes);

        let mut files_transferred = 0usize;
        let mut bytes_transferred = 0u64;
        let mut used_ioring = false;

        for (src, rel, _len) in hot_files {
            let dst = stage_dir.join(&rel);
            let rep = ioring_copy_file(&src, &dst)?;
            files_transferred += 1;
            bytes_transferred = bytes_transferred.saturating_add(rep.bytes_transferred);
            used_ioring |= rep.used_ioring_api;
        }

        let mut store = load_store();
        let mut entry = inspect_game_state(&game.id, &game_root);
        let mut attach_attempted = false;
        let mut attached_physical_path = None;

        if let Some(ref vp) = entry.vhdx_path {
            let p = PathBuf::from(vp);
            if p.is_file() {
                attach_attempted = true;
                if let Ok(phys) = win_virtdisk::try_attach_vhdx(&p) {
                    attached_physical_path = phys;
                }
            }
        }

        entry.hot_stage_dir = Some(stage_dir.to_string_lossy().to_string());
        entry.hot_staged = true;
        entry.hot_staged_bytes = bytes_transferred;
        entry.hot_staged_files = files_transferred;
        entry.ioring_used = used_ioring;
        if let Some(auto) = auto_stage_on_launch {
            entry.auto_stage_on_launch = auto;
        }
        entry.attach_mode = if attached_physical_path.is_some() {
            "VHDX Attached + NVMe IoRing Hot-Tier".to_string()
        } else if used_ioring {
            "NVMe Kernel IoRing Hot-Tier".to_string()
        } else {
            "NVMe Buffered Staging Hot-Tier".to_string()
        };
        let auto_flag = entry.auto_stage_on_launch;
        let mode_label = entry.attach_mode.clone();
        store.games.insert(game.id.clone(), entry);
        save_store(&store)?;

        Ok(HotStageReport {
            game_id: game.id.clone(),
            staged_dir: stage_dir.to_string_lossy().to_string(),
            files_transferred,
            bytes_transferred,
            bytes_formatted: format_bytes(bytes_transferred),
            used_ioring,
            attach_attempted,
            attached_physical_path,
            elapsed_ms: started.elapsed().as_millis() as u64,
            auto_stage_on_launch: auto_flag,
            status_message: format!(
                "{}: staged {} files ({}) onto NVMe hot tier.",
                mode_label,
                files_transferred,
                format_bytes(bytes_transferred)
            ),
        })
    })
    .await
    .map_err(|e| format!("NVMe hot-tier stage task failed: {}", e))?
}

#[tauri::command]
pub async fn unstage_or_detach_vhdx(game_id: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let mut store = load_store();
        let mut freed_bytes = 0u64;
        let mut detached = false;

        if let Some(entry) = store.games.get_mut(&game_id) {
            if let Some(ref vp) = entry.vhdx_path {
                let p = PathBuf::from(vp);
                if p.is_file() {
                    detached = win_virtdisk::try_detach_vhdx(&p).unwrap_or(false);
                }
            }
            if let Some(ref dir) = entry.hot_stage_dir {
                let dp = PathBuf::from(dir);
                if dp.is_dir() {
                    freed_bytes = entry.hot_staged_bytes;
                    let _ = fs::remove_dir_all(&dp);
                }
            }
            entry.hot_staged = false;
            entry.hot_staged_bytes = 0;
            entry.hot_staged_files = 0;
            entry.auto_stage_on_launch = false;
            entry.attach_mode = "Unstaged".to_string();
        }
        save_store(&store)?;
        Ok(format!(
            "Unstaged NVMe hot-tier cache (freed {}, VHDX detached={}).",
            format_bytes(freed_bytes),
            detached
        ))
    })
    .await
    .map_err(|e| format!("VHDX unstage task failed: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("duke_vhdx_test_{}_{}", name, nonce));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn virtdisk_create_and_small_file_pack_work() {
        let dir = tmp_dir("create");
        let small_dir = dir.join("game");
        fs::create_dir_all(&small_dir).unwrap();
        fs::write(small_dir.join("game.exe"), vec![0x4Du8; 2048]).unwrap();
        fs::write(small_dir.join("voice.wem"), vec![0x11u8; 4096]).unwrap();

        let (total_files, small_files, total_bytes, files) = measure_game_files(&small_dir);
        assert_eq!(total_files, 2);
        assert_eq!(small_files, 2);
        assert_eq!(total_bytes, 6144);

        let pack_path = dir.join("test.duke-vhdx-pack");
        let (packed_count, packed_bytes) = pack_small_files_zstd_bundle(&files, &pack_path).unwrap();
        assert_eq!(packed_count, 2);
        assert!(packed_bytes > 0);

        let cap = probe_runtime_capability_internal();
        #[cfg(windows)]
        {
            assert!(cap.virtdisk_dll_loaded);
            assert!(cap.create_virtual_disk);
            let vhdx_path = dir.join("container.vhdx");
            // Validates exact Windows VirtDisk CREATE_VIRTUAL_DISK_PARAMETERS v2 ABI at runtime.
            win_virtdisk::create_vhdx_file(&vhdx_path, 16 * 1024 * 1024).unwrap();
            assert!(vhdx_path.is_file());
        }

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn hot_tier_prioritizes_executables_and_archives_within_budget() {
        let files = vec![
            (PathBuf::from("readme.txt"), "readme.txt".to_string(), 100),
            (PathBuf::from("assets.pak"), "assets.pak".to_string(), 5000),
            (PathBuf::from("game.exe"), "game.exe".to_string(), 2000),
        ];
        let picked = select_hot_files(files, 7050);
        assert_eq!(picked[0].1, "game.exe");
        assert!(picked.iter().any(|f| f.1 == "assets.pak"));
        assert!(!picked.iter().any(|f| f.1 == "readme.txt"));
    }
}
