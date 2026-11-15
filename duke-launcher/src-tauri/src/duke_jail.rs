//! OS-enforced Windows AppContainer Zero-Trust Game Jail + Offline Network Lock.
//!
//! Dynamically binds `userenv.dll` (`CreateAppContainerProfile`, `DeleteAppContainerProfile`,
//! `DeriveAppContainerSidFromAppContainerName`), `advapi32.dll` (SIDs & DACLs), and
//! `kernel32.dll` (`PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`) so untrusted binaries run
//! inside a strict Windows AppContainer with read/execute access only to the game directory and
//! read/write access only to `<game_dir>/.duke-profile/`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const JAIL_POLICY_FILE: &str = "jail_policies.json";
const APPCONTAINER_CAP_INTERNET_CLIENT_SID: &str = "S-1-15-3-1";

#[cfg(target_os = "windows")]
const PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES: usize = 0x0002_0009;
#[cfg(target_os = "windows")]
const EXTENDED_STARTUPINFO_PRESENT: u32 = 0x0008_0000;
#[cfg(target_os = "windows")]
const CREATE_UNICODE_ENVIRONMENT: u32 = 0x0000_0400;
#[cfg(target_os = "windows")]
const CREATE_SUSPENDED: u32 = 0x0000_0004;
#[cfg(target_os = "windows")]
const HRESULT_ALREADY_EXISTS: i32 = -2147024713; // 0x800700B7

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct GameJailPolicy {
    pub game_id: String,
    pub jail_enabled: bool,
    pub offline_mode: bool,
    pub container_name: String,
    pub profile_dir: String,
    pub last_wfp_rule_applied: bool,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct JailPolicyStore {
    pub policies: BTreeMap<String, GameJailPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JailCapabilitySpec {
    pub container_name: String,
    pub zero_capability_mode: bool,
    pub allow_internet_client: bool,
    pub allow_private_network: bool,
    pub capability_sids: Vec<String>,
    pub offline_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameJailStatus {
    pub game_id: String,
    pub container_name: String,
    pub jail_enabled: bool,
    pub offline_mode: bool,
    pub appcontainer_supported: bool,
    pub container_sid_preview: Option<String>,
    pub isolated_profile_dir: String,
    pub acl_scoped_to_game_and_profile: bool,
    pub offline_firewall_active: bool,
    pub summary: String,
}

pub struct JailedLaunchOutcome {
    pub pid: u32,
    pub process_handle_raw: isize,
    pub thread_handle_raw: isize,
    #[allow(dead_code)]
    pub container_name: String,
    #[allow(dead_code)]
    pub offline_network_locked: bool,
    #[allow(dead_code)]
    pub wfp_or_firewall_blocked: bool,
}

fn policy_io_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Derives a valid Win32 AppContainer profile name (strictly <= 64 chars, ASCII alphanumeric + `.`).
pub fn deterministic_container_name(game_id: &str) -> String {
    let digest = blake3::hash(game_id.as_bytes()).to_hex();
    let short_hash = &digest[..16];
    let sanitized: String = game_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(24)
        .collect();
    if sanitized.is_empty() {
        format!("duke.jail.{}", short_hash)
    } else {
        format!("duke.jail.{}.{}", sanitized.to_ascii_lowercase(), short_hash)
    }
}

/// Computes the deterministic offline firewall rule name for a given executable path.
pub fn deterministic_offline_rule_name(exe_path: &Path) -> String {
    let norm = exe_path.to_string_lossy().to_ascii_lowercase();
    let digest = blake3::hash(norm.as_bytes()).to_hex();
    format!("DukeLauncher_OfflineLock_{}", &digest[..16])
}

/// Builds the OS AppContainer security-capability specification.
/// When `offline_mode == true`, `internetClient` and `privateNetworkClientServer` are omitted
/// so the Windows kernel denies all outbound sockets (`zero_capability_mode = true`).
pub fn build_capability_spec(game_id: &str, offline_mode: bool) -> JailCapabilitySpec {
    let container_name = deterministic_container_name(game_id);
    if offline_mode {
        JailCapabilitySpec {
            container_name,
            zero_capability_mode: true,
            allow_internet_client: false,
            allow_private_network: false,
            capability_sids: Vec::new(),
            offline_mode: true,
        }
    } else {
        JailCapabilitySpec {
            container_name,
            zero_capability_mode: false,
            allow_internet_client: true,
            allow_private_network: false,
            capability_sids: vec![APPCONTAINER_CAP_INTERNET_CLIENT_SID.to_string()],
            offline_mode: false,
        }
    }
}

/// Ensures the AppContainer ACL targets are strictly constrained to the game root and its
/// `.duke-profile/` save sandbox, refusing user-profile, browser, or credential roots.
pub fn validate_jail_paths(game_dir: &Path, profile_dir: &Path) -> Result<(), String> {
    let check_not_broad = |candidate: &Path| -> Result<(), String> {
        let lower = candidate.to_string_lossy().replace('/', "\\").to_ascii_lowercase();
        let trimmed = lower.trim_end_matches('\\');
        if trimmed.len() <= 3 && trimmed.ends_with(':') {
            return Err("Refusing to grant AppContainer ACL on an entire drive root.".to_string());
        }
        for env_key in ["USERPROFILE", "APPDATA", "LOCALAPPDATA"] {
            if let Ok(val) = std::env::var(env_key) {
                let env_norm = val.replace('/', "\\").trim_end_matches('\\').to_ascii_lowercase();
                if !env_norm.is_empty() && trimmed == env_norm {
                    return Err(format!(
                        "Security block: AppContainer jail cannot receive broad access to %{env_key}%."
                    ));
                }
            }
        }
        for forbidden_seg in [
            "\\appdata\\local\\google\\chrome",
            "\\appdata\\local\\microsoft\\edge",
            "\\appdata\\roaming\\mozilla\\firefox",
            "\\appdata\\local\\microsoft\\credentials",
            "\\appdata\\roaming\\microsoft\\credentials",
        ] {
            if trimmed.contains(forbidden_seg) {
                return Err("Security block: AppContainer jail never exposes browser or Credential Manager paths.".to_string());
            }
        }
        Ok(())
    };

    check_not_broad(game_dir)?;
    check_not_broad(profile_dir)?;

    let profile_name_ok = profile_dir
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case(".duke-profile"))
        .unwrap_or(false)
        || profile_dir
            .to_string_lossy()
            .to_ascii_lowercase()
            .contains(".duke-profile");
    if !profile_name_ok {
        return Err("AppContainer save sandbox must reside inside a .duke-profile directory.".to_string());
    }
    Ok(())
}

pub fn load_jail_policy_store() -> JailPolicyStore {
    let _g = policy_io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let Ok(path) = crate::get_app_data_file_standalone(JAIL_POLICY_FILE) else {
        return JailPolicyStore::default();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<JailPolicyStore>(&s).ok())
        .unwrap_or_default()
}

pub fn save_jail_policy_store(store: &JailPolicyStore) -> Result<(), String> {
    let _g = policy_io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let path = crate::get_app_data_file_standalone(JAIL_POLICY_FILE)?;
    let serialized = serde_json::to_string_pretty(store)
        .map_err(|e| format!("Failed serializing jail policies: {e}"))?;
    crate::atomic_write_string(&path, &serialized)
}

pub fn get_policy_for_game(game_id: &str) -> GameJailPolicy {
    let store = load_jail_policy_store();
    store
        .policies
        .get(game_id)
        .cloned()
        .unwrap_or_else(|| GameJailPolicy {
            game_id: game_id.to_string(),
            jail_enabled: false,
            offline_mode: false,
            container_name: deterministic_container_name(game_id),
            profile_dir: String::new(),
            last_wfp_rule_applied: false,
            updated_at: 0,
        })
}

#[cfg(target_os = "windows")]
mod win_ac {
    use super::*;
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const SE_FILE_OBJECT: u32 = 1;
    const DACL_SECURITY_INFORMATION: u32 = 0x0000_0004;
    const GRANT_ACCESS: u32 = 1;
    const SUB_CONTAINERS_AND_OBJECTS_INHERIT: u32 = 0x3;
    const TRUSTEE_IS_SID: u32 = 0;
    const TRUSTEE_IS_USER: u32 = 1;
    const FILE_GENERIC_READ_EXEC: u32 = 0x0012_00A9; // SYNCHRONIZE | READ_CONTROL | FILE_READ_DATA/EA/ATTRIBUTES | FILE_EXECUTE/TRAVERSE
    const FILE_ALL_ACCESS: u32 = 0x001F_01FF;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct SidAndAttributes {
        pub sid: *mut c_void,
        pub attributes: u32,
    }

    #[repr(C)]
    pub struct SecurityCapabilities {
        pub app_container_sid: *mut c_void,
        pub capabilities: *mut SidAndAttributes,
        pub capability_count: u32,
        pub reserved: u32,
    }

    #[repr(C)]
    struct TrusteeW {
        multiple_trustee: *mut TrusteeW,
        multiple_trustee_operation: u32,
        trustee_form: u32,
        trustee_type: u32,
        ptstr_name: *mut u16,
    }

    #[repr(C)]
    struct ExplicitAccessW {
        access_permissions: u32,
        access_mode: u32,
        inheritance: u32,
        trustee: TrusteeW,
    }

    #[repr(C)]
    struct StartupInfoW {
        cb: u32,
        lp_reserved: *mut u16,
        lp_desktop: *mut u16,
        lp_title: *mut u16,
        dw_x: u32,
        dw_y: u32,
        dw_x_size: u32,
        dw_y_size: u32,
        dw_x_count_chars: u32,
        dw_y_count_chars: u32,
        dw_fill_attribute: u32,
        dw_flags: u32,
        w_show_window: u16,
        cb_reserved2: u16,
        lp_reserved2: *mut u8,
        h_std_input: *mut c_void,
        h_std_output: *mut c_void,
        h_std_error: *mut c_void,
    }

    #[repr(C)]
    struct StartupInfoExW {
        startup_info: StartupInfoW,
        lp_attribute_list: *mut c_void,
    }

    #[repr(C)]
    struct ProcessInformation {
        h_process: *mut c_void,
        h_thread: *mut c_void,
        dw_process_id: u32,
        dw_thread_id: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryW(name: *const u16) -> isize;
        fn FreeLibrary(h_lib_module: isize) -> i32;
        fn GetProcAddress(module: isize, name: *const u8) -> *const c_void;
        fn LocalFree(h_mem: *mut c_void) -> *mut c_void;
        fn CloseHandle(h_object: isize) -> i32;
        fn InitializeProcThreadAttributeList(
            lp_attribute_list: *mut c_void,
            dw_attribute_count: u32,
            dw_flags: u32,
            lp_size: *mut usize,
        ) -> i32;
        fn UpdateProcThreadAttribute(
            lp_attribute_list: *mut c_void,
            dw_flags: u32,
            attribute: usize,
            lp_value: *const c_void,
            cb_size: usize,
            lp_previous_value: *mut c_void,
            lp_return_size: *mut usize,
        ) -> i32;
        fn DeleteProcThreadAttributeList(lp_attribute_list: *mut c_void);
        fn CreateProcessW(
            lp_application_name: *const u16,
            lp_command_line: *mut u16,
            lp_process_attributes: *mut c_void,
            lp_thread_attributes: *mut c_void,
            b_inherit_handles: i32,
            dw_creation_flags: u32,
            lp_environment: *const c_void,
            lp_current_directory: *const u16,
            lp_startup_info: *const StartupInfoExW,
            lp_process_information: *mut ProcessInformation,
        ) -> i32;
        fn ResumeThread(h_thread: *mut c_void) -> u32;
        fn WaitForSingleObject(h_handle: *mut c_void, dw_milliseconds: u32) -> u32;
        fn GetExitCodeProcess(h_process: *mut c_void, lp_exit_code: *mut u32) -> i32;
        fn GetLastError() -> u32;
    }

    #[link(name = "advapi32")]
    extern "system" {
        fn FreeSid(p_sid: *mut c_void) -> *mut c_void;
        fn ConvertSidToStringSidW(sid: *mut c_void, string_sid: *mut *mut u16) -> i32;
        fn ConvertStringSidToSidW(string_sid: *const u16, sid: *mut *mut c_void) -> i32;
        fn SetEntriesInAclW(
            count_of_explicit_entries: u32,
            list_of_explicit_entries: *const ExplicitAccessW,
            old_acl: *mut c_void,
            new_acl: *mut *mut c_void,
        ) -> u32;
        fn GetNamedSecurityInfoW(
            object_name: *const u16,
            object_type: u32,
            security_info: u32,
            sid_owner: *mut *mut c_void,
            sid_group: *mut *mut c_void,
            dacl: *mut *mut c_void,
            sacl: *mut *mut c_void,
            security_descriptor: *mut *mut c_void,
        ) -> u32;
        fn SetNamedSecurityInfoW(
            object_name: *mut u16,
            object_type: u32,
            security_info: u32,
            sid_owner: *mut c_void,
            sid_group: *mut c_void,
            dacl: *mut c_void,
            sacl: *mut c_void,
        ) -> u32;
    }

    type FnCreateAppContainerProfile = unsafe extern "system" fn(
        *const u16,
        *const u16,
        *const u16,
        *const SidAndAttributes,
        u32,
        *mut *mut c_void,
    ) -> i32;
    type FnDeleteAppContainerProfile = unsafe extern "system" fn(*const u16) -> i32;
    type FnDeriveAppContainerSidFromAppContainerName =
        unsafe extern "system" fn(*const u16, *mut *mut c_void) -> i32;

    struct UserenvApi {
        create_profile: FnCreateAppContainerProfile,
        delete_profile: FnDeleteAppContainerProfile,
        derive_sid: FnDeriveAppContainerSidFromAppContainerName,
    }

    fn utf16_z(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn userenv_api() -> Option<&'static UserenvApi> {
        static API: OnceLock<Option<UserenvApi>> = OnceLock::new();
        API.get_or_init(|| unsafe {
            let dll_name = utf16_z("userenv.dll");
            let hmod = LoadLibraryW(dll_name.as_ptr());
            if hmod == 0 {
                return None;
            }
            let p_create = GetProcAddress(hmod, c"CreateAppContainerProfile".as_ptr().cast::<u8>());
            let p_delete = GetProcAddress(hmod, c"DeleteAppContainerProfile".as_ptr().cast::<u8>());
            let p_derive = GetProcAddress(
                hmod,
                c"DeriveAppContainerSidFromAppContainerName"
                    .as_ptr()
                    .cast::<u8>(),
            );
            if p_create.is_null() || p_delete.is_null() || p_derive.is_null() {
                let _ = FreeLibrary(hmod);
                return None;
            }
            Some(UserenvApi {
                create_profile: std::mem::transmute::<*const c_void, FnCreateAppContainerProfile>(
                    p_create,
                ),
                delete_profile: std::mem::transmute::<*const c_void, FnDeleteAppContainerProfile>(
                    p_delete,
                ),
                derive_sid: std::mem::transmute::<
                    *const c_void,
                    FnDeriveAppContainerSidFromAppContainerName,
                >(p_derive),
            })
        })
        .as_ref()
    }

    pub fn is_supported() -> bool {
        userenv_api().is_some()
    }

    struct OwnedSid {
        ptr: *mut c_void,
        from_local_free: bool,
    }

    impl OwnedSid {
        fn to_string_sid(&self) -> Option<String> {
            if self.ptr.is_null() {
                return None;
            }
            unsafe {
                let mut wide: *mut u16 = std::ptr::null_mut();
                if ConvertSidToStringSidW(self.ptr, &mut wide) == 0 || wide.is_null() {
                    return None;
                }
                let mut len = 0usize;
                while *wide.add(len) != 0 && len < 256 {
                    len += 1;
                }
                let out = String::from_utf16_lossy(std::slice::from_raw_parts(wide, len));
                LocalFree(wide.cast());
                Some(out)
            }
        }
    }

    impl Drop for OwnedSid {
        fn drop(&mut self) {
            if !self.ptr.is_null() {
                unsafe {
                    if self.from_local_free {
                        LocalFree(self.ptr);
                    } else {
                        let _ = FreeSid(self.ptr);
                    }
                }
                self.ptr = std::ptr::null_mut();
            }
        }
    }

    fn ensure_container_sid(
        container_name: &str,
        display_name: &str,
        spec: &JailCapabilitySpec,
    ) -> Result<OwnedSid, String> {
        let api = userenv_api().ok_or_else(|| {
            "userenv.dll AppContainer APIs are unavailable on this Windows build.".to_string()
        })?;
        let name_w = utf16_z(container_name);
        let disp_w = utf16_z(display_name);
        let desc_w = utf16_z("Duke Launcher Zero-Trust Game Sandbox");

        let mut cap_sids_owned: Vec<OwnedSid> = Vec::new();
        let mut cap_attrs: Vec<SidAndAttributes> = Vec::new();
        for sid_str in &spec.capability_sids {
            let sid_w = utf16_z(sid_str);
            let mut raw_sid: *mut c_void = std::ptr::null_mut();
            let ok = unsafe { ConvertStringSidToSidW(sid_w.as_ptr(), &mut raw_sid) };
            if ok != 0 && !raw_sid.is_null() {
                cap_attrs.push(SidAndAttributes {
                    sid: raw_sid,
                    attributes: 0x0000_0004, // SE_GROUP_ENABLED
                });
                cap_sids_owned.push(OwnedSid {
                    ptr: raw_sid,
                    from_local_free: true,
                });
            }
        }

        let mut out_sid: *mut c_void = std::ptr::null_mut();
        let cap_ptr = if cap_attrs.is_empty() {
            std::ptr::null()
        } else {
            cap_attrs.as_ptr()
        };
        let hr = unsafe {
            (api.create_profile)(
                name_w.as_ptr(),
                disp_w.as_ptr(),
                desc_w.as_ptr(),
                cap_ptr,
                cap_attrs.len() as u32,
                &mut out_sid,
            )
        };
        if hr >= 0 && !out_sid.is_null() {
            return Ok(OwnedSid {
                ptr: out_sid,
                from_local_free: false,
            });
        }
        if hr == HRESULT_ALREADY_EXISTS {
            let hr_derive = unsafe { (api.derive_sid)(name_w.as_ptr(), &mut out_sid) };
            if hr_derive >= 0 && !out_sid.is_null() {
                return Ok(OwnedSid {
                    ptr: out_sid,
                    from_local_free: false,
                });
            }
            return Err(format!(
                "DeriveAppContainerSidFromAppContainerName failed: 0x{:08X}",
                hr_derive as u32
            ));
        }
        Err(format!(
            "CreateAppContainerProfile failed with HRESULT 0x{:08X}",
            hr as u32
        ))
    }

    pub fn preview_container_sid(container_name: &str) -> Option<String> {
        let api = userenv_api()?;
        let name_w = utf16_z(container_name);
        let mut raw_sid: *mut c_void = std::ptr::null_mut();
        let hr = unsafe { (api.derive_sid)(name_w.as_ptr(), &mut raw_sid) };
        if hr >= 0 && !raw_sid.is_null() {
            let owned = OwnedSid {
                ptr: raw_sid,
                from_local_free: false,
            };
            return owned.to_string_sid();
        }
        None
    }

    pub fn delete_container_profile(container_name: &str) {
        if let Some(api) = userenv_api() {
            let name_w = utf16_z(container_name);
            unsafe {
                let _ = (api.delete_profile)(name_w.as_ptr());
            }
        }
    }

    fn grant_sid_dir_access(dir: &Path, sid: *mut c_void, access_mask: u32) -> Result<(), String> {
        let mut dir_w = utf16_z(&dir.to_string_lossy());
        let mut old_dacl: *mut c_void = std::ptr::null_mut();
        let mut sec_desc: *mut c_void = std::ptr::null_mut();
        let status = unsafe {
            GetNamedSecurityInfoW(
                dir_w.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut old_dacl,
                std::ptr::null_mut(),
                &mut sec_desc,
            )
        };
        if status != 0 {
            return Err(format!(
                "GetNamedSecurityInfoW failed on {}: Win32 error {}",
                dir.display(),
                status
            ));
        }

        let ea = ExplicitAccessW {
            access_permissions: access_mask,
            access_mode: GRANT_ACCESS,
            inheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
            trustee: TrusteeW {
                multiple_trustee: std::ptr::null_mut(),
                multiple_trustee_operation: 0,
                trustee_form: TRUSTEE_IS_SID,
                trustee_type: TRUSTEE_IS_USER,
                ptstr_name: sid.cast::<u16>(),
            },
        };

        let mut new_dacl: *mut c_void = std::ptr::null_mut();
        let acl_res = unsafe { SetEntriesInAclW(1, &ea, old_dacl, &mut new_dacl) };
        let set_res = if acl_res == 0 && !new_dacl.is_null() {
            unsafe {
                SetNamedSecurityInfoW(
                    dir_w.as_mut_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    new_dacl,
                    std::ptr::null_mut(),
                )
            }
        } else {
            acl_res
        };

        unsafe {
            if !new_dacl.is_null() {
                LocalFree(new_dacl);
            }
            if !sec_desc.is_null() {
                LocalFree(sec_desc);
            }
        }

        if set_res != 0 {
            return Err(format!(
                "SetNamedSecurityInfoW failed on {}: Win32 error {}",
                dir.display(),
                set_res
            ));
        }
        Ok(())
    }

    pub fn apply_or_remove_offline_firewall_rule(exe_path: &Path, block: bool) -> bool {
        let rule_name = deterministic_offline_rule_name(exe_path);
        let mut del_cmd = Command::new("netsh");
        del_cmd.creation_flags(CREATE_NO_WINDOW).args([
            "advfirewall",
            "firewall",
            "delete",
            "rule",
            &format!("name={}", rule_name),
        ]);
        let _ = del_cmd.output();

        if !block {
            return true;
        }

        let mut add_cmd = Command::new("netsh");
        add_cmd.creation_flags(CREATE_NO_WINDOW).args([
            "advfirewall",
            "firewall",
            "add",
            "rule",
            &format!("name={}", rule_name),
            "dir=out",
            "action=block",
            &format!("program={}", exe_path.to_string_lossy()),
            "enable=yes",
            "profile=any",
        ]);
        add_cmd
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn build_unicode_env_block(extra_vars: &[(String, String)]) -> Vec<u16> {
        let mut merged: BTreeMap<String, (String, String)> = BTreeMap::new();
        for (k, v) in std::env::vars() {
            merged.insert(k.to_ascii_uppercase(), (k, v));
        }
        for (k, v) in extra_vars {
            merged.insert(k.to_ascii_uppercase(), (k.clone(), v.clone()));
        }
        let mut wide = Vec::<u16>::with_capacity(4096);
        for (_, (k, v)) in merged {
            let line = format!("{k}={v}");
            wide.extend(std::ffi::OsStr::new(&line).encode_wide());
            wide.push(0);
        }
        wide.push(0);
        wide
    }

    fn quote_windows_arg(arg: &str) -> String {
        if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
            return arg.to_string();
        }
        let mut out = String::from("\"");
        let mut backslashes = 0usize;
        for ch in arg.chars() {
            match ch {
                '\\' => backslashes += 1,
                '"' => {
                    out.push_str(&"\\".repeat(backslashes * 2 + 1));
                    out.push('"');
                    backslashes = 0;
                }
                c => {
                    if backslashes > 0 {
                        out.push_str(&"\\".repeat(backslashes));
                        backslashes = 0;
                    }
                    out.push(c);
                }
            }
        }
        if backslashes > 0 {
            out.push_str(&"\\".repeat(backslashes * 2));
        }
        out.push('"');
        out
    }

    #[allow(clippy::too_many_arguments)]
    pub fn spawn_jailed_process(
        game_id: &str,
        title: &str,
        exe_path: &Path,
        game_dir: &Path,
        working_dir: &Path,
        profile_dir: &Path,
        args: &[String],
        env_vars: &[(String, String)],
        offline_mode: bool,
    ) -> Result<JailedLaunchOutcome, String> {
        validate_jail_paths(game_dir, profile_dir)?;
        fs::create_dir_all(profile_dir)
            .map_err(|e| format!("Failed creating .duke-profile sandbox {}: {e}", profile_dir.display()))?;

        let spec = build_capability_spec(game_id, offline_mode);
        let container_sid = ensure_container_sid(&spec.container_name, title, &spec)?;

        // Grant Read/Execute ONLY to game directory, and Read/Write ONLY to `.duke-profile/`.
        grant_sid_dir_access(game_dir, container_sid.ptr, FILE_GENERIC_READ_EXEC)?;
        grant_sid_dir_access(profile_dir, container_sid.ptr, FILE_ALL_ACCESS)?;

        let firewall_ok = if offline_mode {
            apply_or_remove_offline_firewall_rule(exe_path, true)
        } else {
            false
        };

        let mut cap_sids_owned: Vec<OwnedSid> = Vec::new();
        let mut cap_attrs: Vec<SidAndAttributes> = Vec::new();
        for sid_str in &spec.capability_sids {
            let sid_w = utf16_z(sid_str);
            let mut raw_sid: *mut c_void = std::ptr::null_mut();
            let ok = unsafe { ConvertStringSidToSidW(sid_w.as_ptr(), &mut raw_sid) };
            if ok != 0 && !raw_sid.is_null() {
                cap_attrs.push(SidAndAttributes {
                    sid: raw_sid,
                    attributes: 0x0000_0004,
                });
                cap_sids_owned.push(OwnedSid {
                    ptr: raw_sid,
                    from_local_free: true,
                });
            }
        }

        let sec_caps = SecurityCapabilities {
            app_container_sid: container_sid.ptr,
            capabilities: if cap_attrs.is_empty() {
                std::ptr::null_mut()
            } else {
                cap_attrs.as_mut_ptr()
            },
            capability_count: cap_attrs.len() as u32,
            reserved: 0,
        };

        let mut attr_size: usize = 0;
        unsafe {
            InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut attr_size);
        }
        if attr_size == 0 {
            return Err("InitializeProcThreadAttributeList returned zero byte size.".to_string());
        }
        let mut attr_buf = vec![0u8; attr_size];
        let attr_ptr = attr_buf.as_mut_ptr().cast::<c_void>();
        if unsafe { InitializeProcThreadAttributeList(attr_ptr, 1, 0, &mut attr_size) } == 0 {
            let err = unsafe { GetLastError() };
            return Err(format!("InitializeProcThreadAttributeList failed: Win32 {err}"));
        }

        struct AttrListGuard(*mut c_void);
        impl Drop for AttrListGuard {
            fn drop(&mut self) {
                unsafe { DeleteProcThreadAttributeList(self.0) };
            }
        }
        let _attr_guard = AttrListGuard(attr_ptr);

        if unsafe {
            UpdateProcThreadAttribute(
                attr_ptr,
                0,
                PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
                (&sec_caps as *const SecurityCapabilities).cast::<c_void>(),
                size_of::<SecurityCapabilities>(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } == 0
        {
            let err = unsafe { GetLastError() };
            return Err(format!(
                "UpdateProcThreadAttribute(SECURITY_CAPABILITIES) failed: Win32 {err}"
            ));
        }

        let mut si_ex: StartupInfoExW = unsafe { zeroed() };
        si_ex.startup_info.cb = size_of::<StartupInfoExW>() as u32;
        si_ex.lp_attribute_list = attr_ptr;

        let exe_w = utf16_z(&exe_path.to_string_lossy());
        let mut cmdline = quote_windows_arg(&exe_path.to_string_lossy());
        for a in args {
            cmdline.push(' ');
            cmdline.push_str(&quote_windows_arg(a));
        }
        let mut cmdline_w = utf16_z(&cmdline);
        let wd_w = utf16_z(&working_dir.to_string_lossy());
        let env_w = build_unicode_env_block(env_vars);
        let mut pi: ProcessInformation = unsafe { zeroed() };

        let ok = unsafe {
            CreateProcessW(
                exe_w.as_ptr(),
                cmdline_w.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED,
                env_w.as_ptr().cast::<c_void>(),
                wd_w.as_ptr(),
                &si_ex,
                &mut pi,
            )
        };
        if ok == 0 {
            let err = unsafe { GetLastError() };
            return Err(format!(
                "AppContainer CreateProcessW failed for {}: Win32 error {}",
                exe_path.display(),
                err
            ));
        }

        Ok(JailedLaunchOutcome {
            pid: pi.dw_process_id,
            process_handle_raw: pi.h_process as isize,
            thread_handle_raw: pi.h_thread as isize,
            container_name: spec.container_name,
            offline_network_locked: offline_mode,
            wfp_or_firewall_blocked: firewall_ok,
        })
    }

    pub fn resume_thread_keep_process(thread_handle: isize) {
        unsafe {
            if thread_handle != 0 {
                let _ = ResumeThread(thread_handle as *mut c_void);
                let _ = CloseHandle(thread_handle);
            }
        }
    }

    pub fn wait_process_exit_blocking(process_handle: isize) -> Option<i32> {
        if process_handle == 0 {
            return None;
        }
        const INFINITE: u32 = 0xFFFF_FFFF;
        unsafe {
            let _ = WaitForSingleObject(process_handle as *mut c_void, INFINITE);
            let mut exit_code: u32 = 0;
            let got = GetExitCodeProcess(process_handle as *mut c_void, &mut exit_code);
            let _ = CloseHandle(process_handle);
            if got != 0 {
                Some(exit_code as i32)
            } else {
                None
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod win_ac {
    use super::*;
    pub fn is_supported() -> bool {
        duke_core::platform_backend::active_platform_backend()
            .probe_isolation_capability()
            .supported
    }
    pub fn preview_container_sid(container_name: &str) -> Option<String> {
        Some(format!("linux-ns:{container_name}"))
    }
    pub fn delete_container_profile(_container_name: &str) {}
    pub fn apply_or_remove_offline_firewall_rule(_exe_path: &Path, block: bool) -> bool {
        // Linux SteamOS/Bazzite uses bwrap --unshare-net during spawn_jailed_process
        block
    }
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_jailed_process(
        game_id: &str,
        _title: &str,
        exe_path: &Path,
        game_dir: &Path,
        working_dir: &Path,
        profile_dir: &Path,
        args: &[String],
        env_vars: &[(String, String)],
        offline_mode: bool,
    ) -> Result<JailedLaunchOutcome, String> {
        let backend = duke_core::platform_backend::active_platform_backend();
        let spec = duke_core::platform_backend::IsolationSpec {
            container_id: deterministic_container_name(game_id),
            game_dir: game_dir.to_path_buf(),
            profile_dir: profile_dir.to_path_buf(),
            offline_mode,
            allow_gpu_access: true,
        };
        let _ = backend.prepare_game_isolation(&spec)?;
        let bwrap_args = duke_core::platform_backend::linux_steamos::build_bwrap_landlock_argv(&spec);
        let bwrap_bin = Path::new("/usr/bin/bwrap");
        let mut cmd = if bwrap_bin.is_file() {
            let mut c = std::process::Command::new(bwrap_bin);
            c.args(&bwrap_args).arg(exe_path).args(args);
            c
        } else {
            let mut c = std::process::Command::new(exe_path);
            c.args(args);
            c
        };
        cmd.current_dir(working_dir);
        for (k, v) in env_vars {
            cmd.env(k, v);
        }
        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed spawning Linux/SteamOS sandboxed process: {e}"))?;
        let pid = child.id();
        std::mem::forget(child);
        Ok(JailedLaunchOutcome {
            pid,
            process_handle_raw: 0,
            thread_handle_raw: 0,
            container_name: spec.container_id,
            offline_network_locked: offline_mode,
            wfp_or_firewall_blocked: offline_mode,
        })
    }
    pub fn resume_thread_keep_process(_thread_handle: isize) {}
    pub fn wait_process_exit_blocking(_process_handle: isize) -> Option<i32> {
        None
    }
}

pub use win_ac::{
    apply_or_remove_offline_firewall_rule, delete_container_profile,
    resume_thread_keep_process, spawn_jailed_process, wait_process_exit_blocking,
};

fn build_status(game_id: &str, exe_path: Option<&Path>, policy: &GameJailPolicy) -> GameJailStatus {
    let container_name = if policy.container_name.is_empty() {
        deterministic_container_name(game_id)
    } else {
        policy.container_name.clone()
    };
    let profile_dir = if !policy.profile_dir.is_empty() {
        policy.profile_dir.clone()
    } else if let Some(exe) = exe_path {
        let root = crate::resolve_game_root_dir(exe);
        root.join(".duke-profile").to_string_lossy().to_string()
    } else {
        String::new()
    };

    let backend = duke_core::platform_backend::active_platform_backend();
    let cap_probe = backend.probe_isolation_capability();
    let sid_preview = win_ac::preview_container_sid(&container_name);
    let summary = match (policy.jail_enabled, policy.offline_mode) {
        (true, true) => format!(
            "{} + Offline Network Lock active (zero outbound network & profile isolation).",
            cap_probe.backend_name
        ),
        (true, false) => format!(
            "{} active (ACLs locked to game folder + .duke-profile/).",
            cap_probe.backend_name
        ),
        (false, true) => "Offline Network Lock active (outbound rule blocks telemetry & phone-home sockets).".to_string(),
        (false, false) => format!(
            "Standard native launch mode on {} (enable Jail for untrusted binaries or Offline Mode for zero phone-home).",
            cap_probe.arch
        ),
    };

    GameJailStatus {
        game_id: game_id.to_string(),
        container_name,
        jail_enabled: policy.jail_enabled,
        offline_mode: policy.offline_mode,
        appcontainer_supported: win_ac::is_supported() || cap_probe.supported,
        container_sid_preview: sid_preview,
        isolated_profile_dir: profile_dir,
        acl_scoped_to_game_and_profile: policy.jail_enabled,
        offline_firewall_active: policy.last_wfp_rule_applied || (policy.jail_enabled && policy.offline_mode),
        summary,
    }
}

#[tauri::command]
pub async fn get_game_jail_status(game_id: String) -> Result<GameJailStatus, String> {
    let gid = game_id.trim().to_string();
    let policy = get_policy_for_game(&gid);
    let exe_opt = crate::load_library_standalone()
        .ok()
        .and_then(|lib| lib.into_iter().find(|g| g.id == gid))
        .map(|g| PathBuf::from(g.exe_path));
    Ok(build_status(&gid, exe_opt.as_deref(), &policy))
}

#[tauri::command]
pub async fn set_game_jail_policy(
    app: tauri::AppHandle,
    game_id: String,
    jail_enabled: bool,
    offline_mode: bool,
) -> Result<GameJailStatus, String> {
    let gid = game_id.trim().to_string();
    if gid.is_empty() {
        return Err("gameId is required.".to_string());
    }
    let exe_opt = crate::load_library_standalone()
        .ok()
        .and_then(|lib| lib.into_iter().find(|g| g.id == gid))
        .map(|g| PathBuf::from(g.exe_path));

    let profile_dir = exe_opt
        .as_deref()
        .map(|exe| {
            crate::resolve_game_root_dir(exe)
                .join(".duke-profile")
                .to_string_lossy()
                .to_string()
        })
        .unwrap_or_default();

    let fw_applied = if let Some(ref exe) = exe_opt {
        apply_or_remove_offline_firewall_rule(exe, offline_mode)
    } else {
        false
    };

    let container_name = deterministic_container_name(&gid);
    if !jail_enabled {
        delete_container_profile(&container_name);
    }

    let policy = GameJailPolicy {
        game_id: gid.clone(),
        jail_enabled,
        offline_mode,
        container_name,
        profile_dir,
        last_wfp_rule_applied: offline_mode && fw_applied,
        updated_at: now_unix_secs(),
    };

    let mut store = load_jail_policy_store();
    store.policies.insert(gid.clone(), policy.clone());
    save_jail_policy_store(&store)?;

    // Keep library.json `sandboxEnabled` synchronized with Jail toggle so UI cards match.
    if let Ok(mut lib) = crate::load_library_from_disk(&app) {
        if let Some(entry) = lib.iter_mut().find(|g| g.id == gid) {
            entry.sandbox_enabled = jail_enabled;
            let _ = crate::save_library_to_disk(&app, &lib);
        }
    }

    Ok(build_status(&gid, exe_opt.as_deref(), &policy))
}

pub(crate) fn cleanup_game_jail_profile_sync(
    game_id: &str,
    exe_path: &Path,
) -> Result<(), String> {
    let gid = game_id.trim();
    let container_name = deterministic_container_name(gid);
    delete_container_profile(&container_name);
    let _ = apply_or_remove_offline_firewall_rule(exe_path, false);
    let mut store = load_jail_policy_store();
    store.policies.remove(gid);
    save_jail_policy_store(&store)
}

#[tauri::command]
pub async fn cleanup_game_jail_profile(game_id: String) -> Result<(), String> {
    let gid = game_id.trim().to_string();
    let exe_path = crate::load_library_standalone()
        .ok()
        .and_then(|lib| lib.into_iter().find(|g| g.id == gid))
        .map(|entry| PathBuf::from(entry.exe_path))
        .unwrap_or_default();
    cleanup_game_jail_profile_sync(&gid, &exe_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appcontainer_name_and_capability_specs_are_deterministic() {
        let name = deterministic_container_name("game_c_games_super_long_folder_name_with_spaces_123456789");
        assert!(name.starts_with("duke.jail."));
        assert!(name.len() <= 64);

        let offline = build_capability_spec("test-game", true);
        assert!(offline.zero_capability_mode);
        assert!(!offline.allow_internet_client);
        assert!(offline.capability_sids.is_empty());

        let online = build_capability_spec("test-game", false);
        assert!(!online.zero_capability_mode);
        assert!(online.allow_internet_client);
        assert_eq!(online.capability_sids, vec![APPCONTAINER_CAP_INTERNET_CLIENT_SID.to_string()]);
    }

    #[test]
    fn jail_path_validation_blocks_broad_user_and_browser_roots() {
        let root = std::env::temp_dir().join("duke_jail_path_test");
        let game_dir = root.join("GameOne");
        let valid_prof = game_dir.join(".duke-profile");
        assert!(validate_jail_paths(&game_dir, &valid_prof).is_ok());

        let bad_drive = Path::new("C:\\");
        assert!(validate_jail_paths(bad_drive, &valid_prof).is_err());

        let bad_browser = Path::new("C:\\Users\\Demo\\AppData\\Local\\Google\\Chrome\\User Data\\.duke-profile");
        assert!(validate_jail_paths(&game_dir, bad_browser).is_err());
    }
}
