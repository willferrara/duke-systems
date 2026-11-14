//! Cached Win32 FFI pointers and process scheduling / working-set primitives.
//!
//! All Win32 dynamic symbols (`kernel32.dll`, `psapi.dll`, `winmm.dll`) are
//! resolved at most once per process lifetime into `static OnceLock` pointers.

#[cfg(target_os = "windows")]
mod imp {
    use core::ffi::c_void;
    use std::sync::OnceLock;
    use windows::core::PCSTR;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress, LoadLibraryA};

    type RawFarProc = unsafe extern "system" fn() -> isize;
    type GetCurrentProcessFn = unsafe extern "system" fn() -> *mut c_void;
    type OpenProcessFn = unsafe extern "system" fn(u32, i32, u32) -> *mut c_void;
    type CloseHandleFn = unsafe extern "system" fn(*mut c_void) -> i32;

    #[cfg(feature = "win32-trim")]
    type K32EmptyWorkingSetFn = unsafe extern "system" fn(*mut c_void) -> i32;
    #[cfg(feature = "win32-trim")]
    type SetProcessWorkingSetSizeExFn =
        unsafe extern "system" fn(*mut c_void, usize, usize, u32) -> i32;
    #[cfg(feature = "win32-trim")]
    type K32GetProcessMemoryInfoFn =
        unsafe extern "system" fn(*mut c_void, *mut ProcessMemoryCounters, u32) -> i32;

    #[cfg(feature = "eco-qos")]
    type SetPriorityClassFn = unsafe extern "system" fn(*mut c_void, u32) -> i32;
    #[cfg(feature = "eco-qos")]
    type SetProcessInformationFn =
        unsafe extern "system" fn(*mut c_void, i32, *const c_void, u32) -> i32;
    #[cfg(feature = "eco-qos")]
    type GetCurrentThreadFn = unsafe extern "system" fn() -> *mut c_void;
    #[cfg(feature = "eco-qos")]
    type SetThreadPriorityFn = unsafe extern "system" fn(*mut c_void, i32) -> i32;
    #[cfg(feature = "eco-qos")]
    type TimePeriodFn = unsafe extern "system" fn(u32) -> u32;

    #[cfg(feature = "win32-trim")]
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

    #[cfg(feature = "eco-qos")]
    #[repr(C)]
    struct ProcessPowerThrottlingState {
        version: u32,
        control_mask: u32,
        state_mask: u32,
    }

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    #[cfg(feature = "win32-trim")]
    const PROCESS_SET_QUOTA: u32 = 0x0100;
    #[cfg(feature = "eco-qos")]
    const PROCESS_SET_INFORMATION: u32 = 0x0200;

    #[cfg(feature = "eco-qos")]
    const NORMAL_PRIORITY_CLASS: u32 = 0x0000_0020;
    #[cfg(feature = "eco-qos")]
    const IDLE_PRIORITY_CLASS: u32 = 0x0000_0040;
    #[cfg(feature = "eco-qos")]
    const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
    #[cfg(feature = "eco-qos")]
    const ABOVE_NORMAL_PRIORITY_CLASS: u32 = 0x0000_8000;
    #[cfg(feature = "eco-qos")]
    const THREAD_PRIORITY_HIGHEST: i32 = 2;
    #[cfg(feature = "eco-qos")]
    const PROCESS_POWER_THROTTLING_INFO_CLASS: i32 = 4;
    #[cfg(feature = "eco-qos")]
    const PROCESS_POWER_THROTTLING_CURRENT_VERSION: u32 = 1;
    #[cfg(feature = "eco-qos")]
    const PROCESS_POWER_THROTTLING_EXECUTION_SPEED: u32 = 0x1;

    static FN_GET_CURRENT_PROCESS: OnceLock<Option<GetCurrentProcessFn>> = OnceLock::new();
    static FN_OPEN_PROCESS: OnceLock<Option<OpenProcessFn>> = OnceLock::new();
    static FN_CLOSE_HANDLE: OnceLock<Option<CloseHandleFn>> = OnceLock::new();

    #[cfg(feature = "win32-trim")]
    static FN_K32_EMPTY_WORKING_SET: OnceLock<Option<K32EmptyWorkingSetFn>> = OnceLock::new();
    #[cfg(feature = "win32-trim")]
    static FN_SET_PROCESS_WORKING_SET_SIZE_EX: OnceLock<Option<SetProcessWorkingSetSizeExFn>> =
        OnceLock::new();
    #[cfg(feature = "win32-trim")]
    static FN_K32_GET_PROCESS_MEMORY_INFO: OnceLock<Option<K32GetProcessMemoryInfoFn>> =
        OnceLock::new();

    #[cfg(feature = "eco-qos")]
    static FN_SET_PRIORITY_CLASS: OnceLock<Option<SetPriorityClassFn>> = OnceLock::new();
    #[cfg(feature = "eco-qos")]
    static FN_SET_PROCESS_INFORMATION: OnceLock<Option<SetProcessInformationFn>> = OnceLock::new();
    #[cfg(feature = "eco-qos")]
    static FN_GET_CURRENT_THREAD: OnceLock<Option<GetCurrentThreadFn>> = OnceLock::new();
    #[cfg(feature = "eco-qos")]
    static FN_SET_THREAD_PRIORITY: OnceLock<Option<SetThreadPriorityFn>> = OnceLock::new();
    #[cfg(feature = "eco-qos")]
    static FN_TIME_BEGIN_PERIOD: OnceLock<Option<TimePeriodFn>> = OnceLock::new();
    #[cfg(feature = "eco-qos")]
    static FN_TIME_END_PERIOD: OnceLock<Option<TimePeriodFn>> = OnceLock::new();

    fn resolve_proc(
        dll_cstr: &[u8],
        fallback_dll_cstr: Option<&[u8]>,
        sym_cstr: &[u8],
    ) -> Option<RawFarProc> {
        let try_module = |module_name: &[u8]| -> Option<RawFarProc> {
            unsafe {
                let pc = PCSTR(module_name.as_ptr());
                let hmod = GetModuleHandleA(pc)
                    .ok()
                    .or_else(|| LoadLibraryA(pc).ok())?;
                GetProcAddress(hmod, PCSTR(sym_cstr.as_ptr()))
            }
        };
        try_module(dll_cstr).or_else(|| fallback_dll_cstr.and_then(try_module))
    }

    #[inline]
    fn current_process_handle() -> *mut c_void {
        let func = *FN_GET_CURRENT_PROCESS.get_or_init(|| {
            resolve_proc(b"kernel32.dll\0", None, b"GetCurrentProcess\0")
                .map(|p| unsafe { std::mem::transmute::<RawFarProc, GetCurrentProcessFn>(p) })
        });
        match func {
            Some(f) => unsafe { f() },
            None => (-1isize) as *mut c_void,
        }
    }

    #[inline]
    fn open_process_raw(desired_access: u32, pid: u32) -> Option<*mut c_void> {
        let func = (*FN_OPEN_PROCESS.get_or_init(|| {
            resolve_proc(b"kernel32.dll\0", None, b"OpenProcess\0")
                .map(|p| unsafe { std::mem::transmute::<RawFarProc, OpenProcessFn>(p) })
        }))?;
        let handle = unsafe { func(desired_access, 0, pid) };
        if handle.is_null() || handle == (-1isize) as *mut c_void {
            None
        } else {
            Some(handle)
        }
    }

    #[inline]
    fn close_handle_raw(handle: *mut c_void) {
        if let Some(func) = *FN_CLOSE_HANDLE.get_or_init(|| {
            resolve_proc(b"kernel32.dll\0", None, b"CloseHandle\0")
                .map(|p| unsafe { std::mem::transmute::<RawFarProc, CloseHandleFn>(p) })
        }) {
            unsafe {
                let _ = func(handle);
            }
        }
    }

    #[cfg(feature = "win32-trim")]
    #[inline]
    fn k32_empty_working_set() -> Option<K32EmptyWorkingSetFn> {
        *FN_K32_EMPTY_WORKING_SET.get_or_init(|| {
            resolve_proc(b"kernel32.dll\0", None, b"K32EmptyWorkingSet\0")
                .or_else(|| resolve_proc(b"psapi.dll\0", None, b"EmptyWorkingSet\0"))
                .map(|p| unsafe { std::mem::transmute::<RawFarProc, K32EmptyWorkingSetFn>(p) })
        })
    }

    #[cfg(feature = "win32-trim")]
    #[inline]
    fn set_process_working_set_size_ex() -> Option<SetProcessWorkingSetSizeExFn> {
        *FN_SET_PROCESS_WORKING_SET_SIZE_EX.get_or_init(|| {
            resolve_proc(b"kernel32.dll\0", None, b"SetProcessWorkingSetSizeEx\0").map(|p| unsafe {
                std::mem::transmute::<RawFarProc, SetProcessWorkingSetSizeExFn>(p)
            })
        })
    }

    #[cfg(feature = "win32-trim")]
    #[inline]
    fn k32_get_process_memory_info() -> Option<K32GetProcessMemoryInfoFn> {
        *FN_K32_GET_PROCESS_MEMORY_INFO.get_or_init(|| {
            resolve_proc(b"kernel32.dll\0", None, b"K32GetProcessMemoryInfo\0")
                .or_else(|| resolve_proc(b"psapi.dll\0", None, b"GetProcessMemoryInfo\0"))
                .map(|p| unsafe {
                    std::mem::transmute::<RawFarProc, K32GetProcessMemoryInfoFn>(p)
                })
        })
    }

    #[cfg(feature = "eco-qos")]
    #[inline]
    fn set_priority_class_fn() -> Option<SetPriorityClassFn> {
        *FN_SET_PRIORITY_CLASS.get_or_init(|| {
            resolve_proc(b"kernel32.dll\0", None, b"SetPriorityClass\0")
                .map(|p| unsafe { std::mem::transmute::<RawFarProc, SetPriorityClassFn>(p) })
        })
    }

    #[cfg(feature = "eco-qos")]
    #[inline]
    fn set_process_information_fn() -> Option<SetProcessInformationFn> {
        *FN_SET_PROCESS_INFORMATION.get_or_init(|| {
            resolve_proc(b"kernel32.dll\0", None, b"SetProcessInformation\0")
                .map(|p| unsafe { std::mem::transmute::<RawFarProc, SetProcessInformationFn>(p) })
        })
    }

    #[cfg(feature = "eco-qos")]
    #[inline]
    fn get_current_thread_fn() -> Option<GetCurrentThreadFn> {
        *FN_GET_CURRENT_THREAD.get_or_init(|| {
            resolve_proc(b"kernel32.dll\0", None, b"GetCurrentThread\0")
                .map(|p| unsafe { std::mem::transmute::<RawFarProc, GetCurrentThreadFn>(p) })
        })
    }

    #[cfg(feature = "eco-qos")]
    #[inline]
    fn set_thread_priority_fn() -> Option<SetThreadPriorityFn> {
        *FN_SET_THREAD_PRIORITY.get_or_init(|| {
            resolve_proc(b"kernel32.dll\0", None, b"SetThreadPriority\0")
                .map(|p| unsafe { std::mem::transmute::<RawFarProc, SetThreadPriorityFn>(p) })
        })
    }

    #[cfg(feature = "eco-qos")]
    #[inline]
    fn time_begin_period_fn() -> Option<TimePeriodFn> {
        *FN_TIME_BEGIN_PERIOD.get_or_init(|| {
            resolve_proc(b"winmm.dll\0", None, b"timeBeginPeriod\0")
                .map(|p| unsafe { std::mem::transmute::<RawFarProc, TimePeriodFn>(p) })
        })
    }

    #[cfg(feature = "eco-qos")]
    #[inline]
    fn time_end_period_fn() -> Option<TimePeriodFn> {
        *FN_TIME_END_PERIOD.get_or_init(|| {
            resolve_proc(b"winmm.dll\0", None, b"timeEndPeriod\0")
                .map(|p| unsafe { std::mem::transmute::<RawFarProc, TimePeriodFn>(p) })
        })
    }

    #[cfg(feature = "win32-trim")]
    fn query_working_set_bytes(handle: *mut c_void) -> Option<usize> {
        let get_mem = k32_get_process_memory_info()?;
        let mut counters = ProcessMemoryCounters {
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
        let ok = unsafe { get_mem(handle, &mut counters, counters.cb) };
        if ok != 0 {
            Some(counters.working_set_size)
        } else {
            None
        }
    }

    #[cfg(feature = "win32-trim")]
    pub fn trim_handle_working_set(handle: *mut c_void) -> bool {
        if let Some(set_ws) = set_process_working_set_size_ex() {
            unsafe {
                let _ = set_ws(handle, usize::MAX, usize::MAX, 0);
            }
        }
        if let Some(empty_ws) = k32_empty_working_set() {
            unsafe { empty_ws(handle) != 0 }
        } else {
            false
        }
    }

    #[cfg(feature = "win32-trim")]
    pub fn trim_current_working_set() {
        let handle = current_process_handle();
        let _ = trim_handle_working_set(handle);
    }

    #[cfg(feature = "win32-trim")]
    pub fn get_current_working_set_mb() -> f64 {
        let handle = current_process_handle();
        if let Some(bytes) = query_working_set_bytes(handle) {
            let mb = bytes as f64 / (1024.0 * 1024.0);
            ((mb * 10.0).round() / 10.0).max(0.0)
        } else {
            0.0
        }
    }

    #[cfg(feature = "win32-trim")]
    pub fn get_external_working_set_mb(pid: u32) -> f64 {
        if pid <= 4 {
            return 0.0;
        }
        let Some(handle) = open_process_raw(PROCESS_QUERY_LIMITED_INFORMATION, pid) else {
            return 0.0;
        };
        let mb = query_working_set_bytes(handle)
            .map(|bytes| ((bytes as f64 / (1024.0 * 1024.0)) * 10.0).round() / 10.0)
            .unwrap_or(0.0)
            .max(0.0);
        close_handle_raw(handle);
        mb
    }

    #[cfg(feature = "win32-trim")]
    pub fn trim_external_pid(pid: u32) -> Result<f64, String> {
        if pid <= 4 {
            return Err("Cannot trim kernel/system idle PID.".to_string());
        }
        let handle = open_process_raw(PROCESS_SET_QUOTA | PROCESS_QUERY_LIMITED_INFORMATION, pid)
            .ok_or_else(|| format!("Access denied opening PID {} for working-set trim.", pid))?;
        let before = query_working_set_bytes(handle);
        let trimmed = trim_handle_working_set(handle);
        let after = query_working_set_bytes(handle);
        close_handle_raw(handle);

        if !trimmed {
            return Err(format!("Failed to trim working set for PID {}.", pid));
        }

        let freed_mb = match (before, after) {
            (Some(b), Some(a)) => ((b.saturating_sub(a) as f64 / (1024.0 * 1024.0)) * 10.0).round() / 10.0,
            _ => 0.0,
        };
        Ok(freed_mb)
    }

    #[cfg(feature = "eco-qos")]
    fn apply_power_throttling(handle: *mut c_void, enable_eco: bool) -> bool {
        let Some(set_info) = set_process_information_fn() else {
            return false;
        };
        let state = ProcessPowerThrottlingState {
            version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            control_mask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            state_mask: if enable_eco {
                PROCESS_POWER_THROTTLING_EXECUTION_SPEED
            } else {
                0
            },
        };
        unsafe {
            set_info(
                handle,
                PROCESS_POWER_THROTTLING_INFO_CLASS,
                (&state as *const ProcessPowerThrottlingState).cast(),
                std::mem::size_of::<ProcessPowerThrottlingState>() as u32,
            ) != 0
        }
    }

    #[cfg(feature = "eco-qos")]
    fn set_handle_priority(handle: *mut c_void, priority_class: u32) -> bool {
        let Some(set_pri) = set_priority_class_fn() else {
            return false;
        };
        unsafe { set_pri(handle, priority_class) != 0 }
    }

    #[cfg(feature = "eco-qos")]
    pub fn enable_eco_qos_mode() {
        let handle = current_process_handle();
        let _ = set_handle_priority(handle, BELOW_NORMAL_PRIORITY_CLASS);
        let _ = apply_power_throttling(handle, true);
    }

    #[cfg(feature = "eco-qos")]
    pub fn enable_idle_eco_qos_mode() {
        let handle = current_process_handle();
        let _ = set_handle_priority(handle, IDLE_PRIORITY_CLASS);
        let _ = apply_power_throttling(handle, true);
    }

    #[cfg(feature = "eco-qos")]
    pub fn restore_normal_priority_mode() {
        let handle = current_process_handle();
        let _ = set_handle_priority(handle, NORMAL_PRIORITY_CLASS);
        let _ = apply_power_throttling(handle, false);
    }

    #[cfg(feature = "eco-qos")]
    pub fn set_current_thread_high_priority() {
        let thread = match get_current_thread_fn() {
            Some(f) => unsafe { f() },
            None => (-2isize) as *mut c_void,
        };
        if let Some(set_tp) = set_thread_priority_fn() {
            unsafe {
                let _ = set_tp(thread, THREAD_PRIORITY_HIGHEST);
            }
        }
    }

    #[cfg(feature = "eco-qos")]
    pub fn enable_realtime_input_mode() {
        if let Some(begin) = time_begin_period_fn() {
            unsafe {
                let _ = begin(1);
            }
        }
        let handle = current_process_handle();
        let _ = set_handle_priority(handle, ABOVE_NORMAL_PRIORITY_CLASS);
        let _ = apply_power_throttling(handle, false);
        set_current_thread_high_priority();
    }

    #[cfg(feature = "eco-qos")]
    pub fn disable_realtime_input_timer() {
        if let Some(end) = time_end_period_fn() {
            unsafe {
                let _ = end(1);
            }
        }
    }

    #[cfg(feature = "eco-qos")]
    pub fn boost_external_pid(pid: u32) -> Result<(), String> {
        if pid <= 4 {
            return Err("Cannot boost kernel/system idle PID.".to_string());
        }
        let handle = open_process_raw(
            PROCESS_SET_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION,
            pid,
        )
        .ok_or_else(|| format!("Access denied opening PID {} for P-core boost.", pid))?;

        let pri_ok = set_handle_priority(handle, ABOVE_NORMAL_PRIORITY_CLASS);
        let _ = apply_power_throttling(handle, false);
        close_handle_raw(handle);

        if pri_ok {
            Ok(())
        } else {
            Err(format!("Failed elevating priority on PID {}.", pid))
        }
    }

    #[cfg(feature = "eco-qos")]
    pub fn jail_external_pid_eco_qos(pid: u32) -> Result<f64, String> {
        if pid <= 4 || pid == std::process::id() {
            return Err("Cannot Eco-Jail system idle/kernel or current process.".to_string());
        }

        #[cfg(feature = "win32-trim")]
        let desired =
            PROCESS_SET_INFORMATION | PROCESS_SET_QUOTA | PROCESS_QUERY_LIMITED_INFORMATION;
        #[cfg(not(feature = "win32-trim"))]
        let desired = PROCESS_SET_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION;

        let handle = open_process_raw(desired, pid)
            .ok_or_else(|| format!("Access denied opening PID {} for EcoQoS.", pid))?;

        #[cfg(feature = "win32-trim")]
        let before = query_working_set_bytes(handle);

        let pri_ok = set_handle_priority(handle, BELOW_NORMAL_PRIORITY_CLASS);
        let throttle_ok = apply_power_throttling(handle, true);

        #[cfg(feature = "win32-trim")]
        let freed_mb = {
            let trimmed = trim_handle_working_set(handle);
            let after = query_working_set_bytes(handle);
            if trimmed {
                match (before, after) {
                    (Some(b), Some(a)) => {
                        ((b.saturating_sub(a) as f64 / (1024.0 * 1024.0)) * 10.0).round() / 10.0
                    }
                    _ => 0.0,
                }
            } else {
                0.0
            }
        };
        #[cfg(not(feature = "win32-trim"))]
        let freed_mb = 0.0;

        close_handle_raw(handle);

        if !pri_ok && !throttle_ok {
            return Err(format!(
                "Windows refused EcoQoS update for PID {}. Try running elevated.",
                pid
            ));
        }

        Ok(freed_mb)
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    #[cfg(feature = "win32-trim")]
    pub fn trim_current_working_set() {}

    #[cfg(feature = "win32-trim")]
    pub fn get_current_working_set_mb() -> f64 {
        0.0
    }

    #[cfg(feature = "win32-trim")]
    pub fn get_external_working_set_mb(_pid: u32) -> f64 {
        0.0
    }

    #[cfg(feature = "win32-trim")]
    pub fn trim_external_pid(_pid: u32) -> Result<f64, String> {
        Ok(0.0)
    }

    #[cfg(feature = "eco-qos")]
    pub fn enable_eco_qos_mode() {}

    #[cfg(feature = "eco-qos")]
    pub fn enable_idle_eco_qos_mode() {}

    #[cfg(feature = "eco-qos")]
    pub fn restore_normal_priority_mode() {}

    #[cfg(feature = "eco-qos")]
    pub fn set_current_thread_high_priority() {}

    #[cfg(feature = "eco-qos")]
    pub fn enable_realtime_input_mode() {}

    #[cfg(feature = "eco-qos")]
    pub fn disable_realtime_input_timer() {}

    #[cfg(feature = "eco-qos")]
    pub fn boost_external_pid(_pid: u32) -> Result<(), String> {
        Ok(())
    }

    #[cfg(feature = "eco-qos")]
    pub fn jail_external_pid_eco_qos(_pid: u32) -> Result<f64, String> {
        Ok(0.0)
    }
}

#[cfg(target_os = "windows")]
mod ext_ffi {
    use core::ffi::c_void;
    use std::sync::OnceLock;
    use windows::Win32::Foundation::{CloseHandle, HMODULE};
    use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress, LoadLibraryA};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_ACCESS_RIGHTS};
    use windows::core::PCSTR;

    const PROCESS_SUSPEND_RESUME: u32 = 0x0800;

    type NtProcessFn = unsafe extern "system" fn(*mut c_void) -> i32;
    type XInputGetStateFn = unsafe extern "system" fn(u32, *mut XInputStateRaw) -> u32;
    type GetAsyncKeyStateFn = unsafe extern "system" fn(i32) -> i16;
    type RegOpenKeyExWFn =
        unsafe extern "system" fn(*mut c_void, *const u16, u32, u32, *mut *mut c_void) -> i32;
    type RegQueryValueExWFn = unsafe extern "system" fn(
        *mut c_void,
        *const u16,
        *mut u32,
        *mut u32,
        *mut u8,
        *mut u32,
    ) -> i32;
    type RegEnumKeyExWFn = unsafe extern "system" fn(
        *mut c_void,
        u32,
        *mut u16,
        *mut u32,
        *mut u32,
        *mut u16,
        *mut u32,
        *mut c_void,
    ) -> i32;
    type RegCloseKeyFn = unsafe extern "system" fn(*mut c_void) -> i32;
    type CreateHardLinkWFn =
        unsafe extern "system" fn(*const u16, *const u16, *const c_void) -> i32;

    static NT_SUSPEND_PROCESS: OnceLock<Option<NtProcessFn>> = OnceLock::new();
    static NT_RESUME_PROCESS: OnceLock<Option<NtProcessFn>> = OnceLock::new();
    static XINPUT_GET_STATE: OnceLock<Option<XInputGetStateFn>> = OnceLock::new();
    static GET_ASYNC_KEY_STATE: OnceLock<Option<GetAsyncKeyStateFn>> = OnceLock::new();
    static ADVAPI_REG: OnceLock<Option<AdvapiRegProcs>> = OnceLock::new();
    static CREATE_HARD_LINK_W: OnceLock<Option<CreateHardLinkWFn>> = OnceLock::new();

    #[derive(Clone, Copy)]
    struct AdvapiRegProcs {
        open_key: RegOpenKeyExWFn,
        query_value: RegQueryValueExWFn,
        enum_key: RegEnumKeyExWFn,
        close_key: RegCloseKeyFn,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct XInputGamepadRaw {
        buttons: u16,
        left_trigger: u8,
        right_trigger: u8,
        thumb_lx: i16,
        thumb_ly: i16,
        thumb_rx: i16,
        thumb_ry: i16,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct XInputStateRaw {
        packet_number: u32,
        gamepad: XInputGamepadRaw,
    }

    fn resolve_ntdll_proc(name: &'static [u8]) -> Option<NtProcessFn> {
        unsafe {
            let hmod = GetModuleHandleA(PCSTR::from_raw(c"ntdll.dll".as_ptr().cast())).ok()?;
            let proc = GetProcAddress(hmod, PCSTR::from_raw(name.as_ptr()))?;
            Some(std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                NtProcessFn,
            >(proc))
        }
    }

    fn resolve_xinput() -> Option<XInputGetStateFn> {
        unsafe {
            let hmod: HMODULE =
                LoadLibraryA(PCSTR::from_raw(c"xinput1_4.dll".as_ptr().cast()))
                    .or_else(|_| LoadLibraryA(PCSTR::from_raw(c"xinput1_3.dll".as_ptr().cast())))
                    .or_else(|_| LoadLibraryA(PCSTR::from_raw(c"xinput9_1_0.dll".as_ptr().cast())))
                    .ok()?;
            // Ordinal 100 (`XInputGetStateEx`) exposes the Xbox Guide button bit (0x0400).
            let proc = GetProcAddress(hmod, PCSTR::from_raw(100usize as *const u8))
                .or_else(|| GetProcAddress(hmod, PCSTR::from_raw(c"XInputGetState".as_ptr().cast())))?;
            Some(std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                XInputGetStateFn,
            >(proc))
        }
    }

    fn resolve_get_async_key_state() -> Option<GetAsyncKeyStateFn> {
        unsafe {
            let hmod = LoadLibraryA(PCSTR::from_raw(c"user32.dll".as_ptr().cast())).ok()?;
            let proc =
                GetProcAddress(hmod, PCSTR::from_raw(c"GetAsyncKeyState".as_ptr().cast()))?;
            Some(std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                GetAsyncKeyStateFn,
            >(proc))
        }
    }

    fn resolve_advapi_reg() -> Option<AdvapiRegProcs> {
        unsafe {
            let hmod = LoadLibraryA(PCSTR::from_raw(c"advapi32.dll".as_ptr().cast())).ok()?;
            let open_p = GetProcAddress(hmod, PCSTR::from_raw(c"RegOpenKeyExW".as_ptr().cast()))?;
            let query_p =
                GetProcAddress(hmod, PCSTR::from_raw(c"RegQueryValueExW".as_ptr().cast()))?;
            let enum_p = GetProcAddress(hmod, PCSTR::from_raw(c"RegEnumKeyExW".as_ptr().cast()))?;
            let close_p = GetProcAddress(hmod, PCSTR::from_raw(c"RegCloseKey".as_ptr().cast()))?;
            Some(AdvapiRegProcs {
                open_key: std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    RegOpenKeyExWFn,
                >(open_p),
                query_value: std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    RegQueryValueExWFn,
                >(query_p),
                enum_key: std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    RegEnumKeyExWFn,
                >(enum_p),
                close_key: std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    RegCloseKeyFn,
                >(close_p),
            })
        }
    }

    /// Suspends all threads of `pid` via `ntdll!NtSuspendProcess` and trims its
    /// physical working set to reclaim gigabytes of RAM while frozen.
    pub fn suspend_external_pid(pid: u32) -> Result<f64, String> {
        if pid == 0 {
            return Err("Invalid PID 0".to_string());
        }
        let suspend_fn = (*NT_SUSPEND_PROCESS
            .get_or_init(|| resolve_ntdll_proc(b"NtSuspendProcess\0")))
        .ok_or_else(|| "NtSuspendProcess unavailable in ntdll.dll".to_string())?;

        let handle = unsafe { OpenProcess(PROCESS_ACCESS_RIGHTS(PROCESS_SUSPEND_RESUME), false, pid) }
            .map_err(|e| format!("OpenProcess(PROCESS_SUSPEND_RESUME) failed: {e}"))?;

        let status = unsafe { suspend_fn(handle.0) };
        unsafe {
            let _ = CloseHandle(handle);
        }

        if status < 0 {
            return Err(format!("NtSuspendProcess returned NTSTATUS 0x{:08X}", status as u32));
        }

        #[cfg(feature = "win32-trim")]
        let freed_mb = super::imp::trim_external_pid(pid).unwrap_or(0.0);
        #[cfg(not(feature = "win32-trim"))]
        let freed_mb = 0.0;

        Ok(freed_mb)
    }

    /// Resumes all threads of a suspended process via `ntdll!NtResumeProcess`.
    pub fn resume_external_pid(pid: u32) -> Result<(), String> {
        if pid == 0 {
            return Err("Invalid PID 0".to_string());
        }
        let resume_fn = (*NT_RESUME_PROCESS
            .get_or_init(|| resolve_ntdll_proc(b"NtResumeProcess\0")))
        .ok_or_else(|| "NtResumeProcess unavailable in ntdll.dll".to_string())?;

        let handle = unsafe { OpenProcess(PROCESS_ACCESS_RIGHTS(PROCESS_SUSPEND_RESUME), false, pid) }
            .map_err(|e| format!("OpenProcess(PROCESS_SUSPEND_RESUME) failed: {e}"))?;

        let status = unsafe { resume_fn(handle.0) };
        unsafe {
            let _ = CloseHandle(handle);
        }

        if status < 0 {
            return Err(format!("NtResumeProcess returned NTSTATUS 0x{:08X}", status as u32));
        }
        Ok(())
    }

    /// Polls connected XInput controllers (slots 0..4) using `xinput1_4.dll`.
    pub fn poll_xinput_state() -> super::XInputPadState {
        let Some(get_state) = *XINPUT_GET_STATE.get_or_init(resolve_xinput) else {
            return super::XInputPadState::default();
        };

        for user_idx in 0..4u32 {
            let mut raw = XInputStateRaw::default();
            let err = unsafe { get_state(user_idx, &mut raw) };
            if err == 0 {
                let b = raw.gamepad.buttons;
                return super::XInputPadState {
                    connected: true,
                    packet_number: raw.packet_number,
                    dpad_up: (b & 0x0001) != 0,
                    dpad_down: (b & 0x0002) != 0,
                    dpad_left: (b & 0x0004) != 0,
                    dpad_right: (b & 0x0008) != 0,
                    start: (b & 0x0010) != 0,
                    back: (b & 0x0020) != 0,
                    lb: (b & 0x0100) != 0,
                    rb: (b & 0x0200) != 0,
                    a: (b & 0x1000) != 0,
                    b: (b & 0x2000) != 0,
                    x: (b & 0x4000) != 0,
                    y: (b & 0x8000) != 0,
                    guide: (b & 0x0400) != 0,
                    thumb_lx: raw.gamepad.thumb_lx,
                    thumb_ly: raw.gamepad.thumb_ly,
                };
            }
        }
        super::XInputPadState::default()
    }

    /// Returns true if virtual key code `vk` is currently held down.
    pub fn is_vk_down(vk: i32) -> bool {
        let Some(f) = *GET_ASYNC_KEY_STATE.get_or_init(resolve_get_async_key_state) else {
            return false;
        };
        unsafe { (f(vk) as u16 & 0x8000) != 0 }
    }

    fn to_wide_null(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(core::iter::once(0)).collect()
    }

    fn open_reg_key(hkey_root: usize, subkey: &str) -> Option<(*mut c_void, AdvapiRegProcs)> {
        let procs = (*ADVAPI_REG.get_or_init(resolve_advapi_reg))?;
        let sub_w = to_wide_null(subkey);
        let mut hkey: *mut c_void = core::ptr::null_mut();
        const KEY_READ: u32 = 0x20019;
        const KEY_WOW64_32KEY: u32 = 0x0200;
        let rc = unsafe {
            (procs.open_key)(
                hkey_root as *mut c_void,
                sub_w.as_ptr(),
                0,
                KEY_READ,
                &mut hkey,
            )
        };
        if rc == 0 && !hkey.is_null() {
            return Some((hkey, procs));
        }
        let rc32 = unsafe {
            (procs.open_key)(
                hkey_root as *mut c_void,
                sub_w.as_ptr(),
                0,
                KEY_READ | KEY_WOW64_32KEY,
                &mut hkey,
            )
        };
        if rc32 == 0 && !hkey.is_null() {
            return Some((hkey, procs));
        }
        None
    }

    /// Reads a string (`REG_SZ` / `REG_EXPAND_SZ`) value from the Windows Registry.
    /// `use_hklm = false` reads `HKCU`; `use_hklm = true` reads `HKLM`.
    pub fn reg_read_string(use_hklm: bool, subkey: &str, value_name: &str) -> Option<String> {
        const HKEY_CURRENT_USER: usize = 0x8000_0001;
        const HKEY_LOCAL_MACHINE: usize = 0x8000_0002;
        const REG_SZ: u32 = 1;
        const REG_EXPAND_SZ: u32 = 2;

        let root = if use_hklm { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER };
        let (hkey, procs) = open_reg_key(root, subkey)?;
        let val_w = to_wide_null(value_name);
        let mut reg_type: u32 = 0;
        let mut bytes_len: u32 = 0;

        let probe = unsafe {
            (procs.query_value)(
                hkey,
                val_w.as_ptr(),
                core::ptr::null_mut(),
                &mut reg_type,
                core::ptr::null_mut(),
                &mut bytes_len,
            )
        };

        if probe != 0 || bytes_len == 0 || (reg_type != REG_SZ && reg_type != REG_EXPAND_SZ) {
            unsafe {
                let _ = (procs.close_key)(hkey);
            }
            return None;
        }

        let mut buf = vec![0u8; bytes_len as usize];
        let rc = unsafe {
            (procs.query_value)(
                hkey,
                val_w.as_ptr(),
                core::ptr::null_mut(),
                &mut reg_type,
                buf.as_mut_ptr(),
                &mut bytes_len,
            )
        };
        unsafe {
            let _ = (procs.close_key)(hkey);
        }
        if rc != 0 {
            return None;
        }

        let u16_count = (bytes_len as usize) / 2;
        let mut wide = Vec::with_capacity(u16_count);
        for chunk in buf[..u16_count * 2].as_chunks::<2>().0 {
            let ch = u16::from_le_bytes([chunk[0], chunk[1]]);
            if ch == 0 {
                break;
            }
            wide.push(ch);
        }
        Some(String::from_utf16_lossy(&wide))
    }

    /// Enumerates direct child subkey names under an `HKCU` or `HKLM` Registry key.
    pub fn reg_enum_subkeys(use_hklm: bool, subkey: &str) -> Vec<String> {
        const HKEY_CURRENT_USER: usize = 0x8000_0001;
        const HKEY_LOCAL_MACHINE: usize = 0x8000_0002;
        let root = if use_hklm { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER };
        let Some((hkey, procs)) = open_reg_key(root, subkey) else {
            return Vec::new();
        };

        let mut out = Vec::new();
        let mut idx = 0u32;
        let mut name_buf = [0u16; 512];

        loop {
            let mut name_len = name_buf.len() as u32;
            let rc = unsafe {
                (procs.enum_key)(
                    hkey,
                    idx,
                    name_buf.as_mut_ptr(),
                    &mut name_len,
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                )
            };
            if rc != 0 {
                break;
            }
            out.push(String::from_utf16_lossy(&name_buf[..name_len as usize]));
            idx += 1;
        }

        unsafe {
            let _ = (procs.close_key)(hkey);
        }
        out
    }

    /// Finds the first running process PID whose executable filename matches `exe_name` (case-insensitive).
    pub fn find_pid_by_exe_name(exe_name: &str) -> Option<u32> {
        let target = exe_name.trim();
        if target.is_empty() {
            return None;
        }
        type CreateSnapFn = unsafe extern "system" fn(u32, u32) -> *mut c_void;
        type ProcessWalkFn = unsafe extern "system" fn(*mut c_void, *mut ProcessEntry32W) -> i32;

        #[repr(C)]
        struct ProcessEntry32W {
            dw_size: u32,
            cnt_usage: u32,
            th32_process_id: u32,
            th32_default_heap_id: usize,
            th32_module_id: u32,
            cnt_threads: u32,
            th32_parent_process_id: u32,
            pc_pri_class_base: i32,
            dw_flags: u32,
            sz_exe_file: [u16; 260],
        }

        let k32 = unsafe {
            GetModuleHandleA(PCSTR::from_raw(c"kernel32.dll".as_ptr().cast())).ok()?
        };
        let snap_p = unsafe {
            GetProcAddress(
                k32,
                PCSTR::from_raw(c"CreateToolhelp32Snapshot".as_ptr().cast()),
            )?
        };
        let first_p = unsafe {
            GetProcAddress(k32, PCSTR::from_raw(c"Process32FirstW".as_ptr().cast()))?
        };
        let next_p = unsafe {
            GetProcAddress(k32, PCSTR::from_raw(c"Process32NextW".as_ptr().cast()))?
        };

        let create_snap: CreateSnapFn =
            unsafe { std::mem::transmute::<_, CreateSnapFn>(snap_p) };
        let proc_first: ProcessWalkFn =
            unsafe { std::mem::transmute::<_, ProcessWalkFn>(first_p) };
        let proc_next: ProcessWalkFn =
            unsafe { std::mem::transmute::<_, ProcessWalkFn>(next_p) };

        let snap = unsafe { create_snap(0x0000_0002, 0) };
        if snap.is_null() || snap == (-1isize as *mut c_void) {
            return None;
        }

        let mut pe = ProcessEntry32W {
            dw_size: core::mem::size_of::<ProcessEntry32W>() as u32,
            cnt_usage: 0,
            th32_process_id: 0,
            th32_default_heap_id: 0,
            th32_module_id: 0,
            cnt_threads: 0,
            th32_parent_process_id: 0,
            pc_pri_class_base: 0,
            dw_flags: 0,
            sz_exe_file: [0u16; 260],
        };

        let mut found = None;
        if unsafe { proc_first(snap, &mut pe) } != 0 {
            loop {
                let len = pe
                    .sz_exe_file
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(pe.sz_exe_file.len());
                let name = String::from_utf16_lossy(&pe.sz_exe_file[..len]);
                if name.eq_ignore_ascii_case(target) && pe.th32_process_id != 0 {
                    found = Some(pe.th32_process_id);
                    break;
                }
                if unsafe { proc_next(snap, &mut pe) } == 0 {
                    break;
                }
            }
        }

        unsafe {
            let _ = CloseHandle(windows::Win32::Foundation::HANDLE(snap));
        }
        found
    }

    type CreateJobObjectWFn = unsafe extern "system" fn(*const c_void, *const u16) -> *mut c_void;
    type AssignProcessToJobObjectFn = unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32;
    type QueryInformationJobObjectFn =
        unsafe extern "system" fn(*mut c_void, u32, *mut c_void, u32, *mut u32) -> i32;
    type OpenProcessFn = unsafe extern "system" fn(u32, i32, u32) -> *mut c_void;

    struct JobProcs {
        create_job_object_w: CreateJobObjectWFn,
        assign_process_to_job_object: AssignProcessToJobObjectFn,
        query_information_job_object: QueryInformationJobObjectFn,
        open_process: OpenProcessFn,
    }

    fn job_procs() -> Option<&'static JobProcs> {
        use std::sync::OnceLock;
        use windows::core::PCSTR;
        use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

        static PROCS: OnceLock<Option<JobProcs>> = OnceLock::new();
        PROCS
            .get_or_init(|| {
                let k32 = unsafe {
                    GetModuleHandleA(PCSTR::from_raw(c"kernel32.dll".as_ptr().cast())).ok()?
                };
                let create_p = unsafe {
                    GetProcAddress(k32, PCSTR::from_raw(c"CreateJobObjectW".as_ptr().cast()))?
                };
                let assign_p = unsafe {
                    GetProcAddress(
                        k32,
                        PCSTR::from_raw(c"AssignProcessToJobObject".as_ptr().cast()),
                    )?
                };
                let query_p = unsafe {
                    GetProcAddress(
                        k32,
                        PCSTR::from_raw(c"QueryInformationJobObject".as_ptr().cast()),
                    )?
                };
                let open_p = unsafe {
                    GetProcAddress(k32, PCSTR::from_raw(c"OpenProcess".as_ptr().cast()))?
                };
                unsafe {
                    Some(JobProcs {
                        create_job_object_w: std::mem::transmute::<
                            unsafe extern "system" fn() -> isize,
                            CreateJobObjectWFn,
                        >(create_p),
                        assign_process_to_job_object: std::mem::transmute::<
                            unsafe extern "system" fn() -> isize,
                            AssignProcessToJobObjectFn,
                        >(assign_p),
                        query_information_job_object: std::mem::transmute::<
                            unsafe extern "system" fn() -> isize,
                            QueryInformationJobObjectFn,
                        >(query_p),
                        open_process: std::mem::transmute::<
                            unsafe extern "system" fn() -> isize,
                            OpenProcessFn,
                        >(open_p),
                    })
                }
            })
            .as_ref()
    }

    #[repr(C)]
    struct JobObjectBasicProcessIdListStack {
        number_of_assigned_processes: u32,
        number_of_process_ids_in_list: u32,
        process_id_list: [usize; 128],
    }

    /// RAII wrapper around a Win32 Job Object handle used to track entire game process trees
    /// (including secondary launchers/bootstrappers and spawned child processes).
    pub struct ProcessTreeJob {
        handle: *mut c_void,
    }

    unsafe impl Send for ProcessTreeJob {}
    unsafe impl Sync for ProcessTreeJob {}

    impl Drop for ProcessTreeJob {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                unsafe {
                    let _ = windows::Win32::Foundation::CloseHandle(
                        windows::Win32::Foundation::HANDLE(self.handle),
                    );
                }
                self.handle = std::ptr::null_mut();
            }
        }
    }

    impl ProcessTreeJob {
        /// Creates an anonymous Win32 Job Object.
        pub fn create() -> Result<Self, String> {
            let procs = job_procs()
                .ok_or_else(|| "kernel32 Job Object APIs are unavailable".to_string())?;
            let handle = unsafe { (procs.create_job_object_w)(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                return Err(format!(
                    "CreateJobObjectW failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
            Ok(Self { handle })
        }

        /// Assigns an existing process ID (and its future descendant tree) to this Job Object.
        pub fn assign_pid(&self, pid: u32) -> Result<(), String> {
            if pid == 0 {
                return Err("Cannot assign PID 0 to Job Object".to_string());
            }
            let procs = job_procs()
                .ok_or_else(|| "kernel32 Job Object APIs are unavailable".to_string())?;
            // AssignProcessToJobObject requires PROCESS_SET_QUOTA (0x0100) | PROCESS_TERMINATE (0x0001).
            const PROCESS_SET_QUOTA_AND_TERMINATE: u32 = 0x0100 | 0x0001;
            let proc_handle =
                unsafe { (procs.open_process)(PROCESS_SET_QUOTA_AND_TERMINATE, 0, pid) };
            if proc_handle.is_null() {
                return Err(format!(
                    "OpenProcess(pid={pid}) failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let ok = unsafe { (procs.assign_process_to_job_object)(self.handle, proc_handle) };
            let err = if ok == 0 {
                Some(std::io::Error::last_os_error())
            } else {
                None
            };
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(
                    windows::Win32::Foundation::HANDLE(proc_handle),
                );
            }
            if let Some(e) = err {
                return Err(format!("AssignProcessToJobObject(pid={pid}) failed: {e}"));
            }
            Ok(())
        }

        /// Queries active process IDs currently tracked inside this Job Object.
        pub fn active_pids(&self) -> Vec<u32> {
            let Some(procs) = job_procs() else {
                return Vec::new();
            };
            const JOB_OBJECT_BASIC_PROCESS_ID_LIST: u32 = 3;
            let mut stack_buf = JobObjectBasicProcessIdListStack {
                number_of_assigned_processes: 0,
                number_of_process_ids_in_list: 0,
                process_id_list: [0usize; 128],
            };
            let mut returned_len: u32 = 0;
            let ok = unsafe {
                (procs.query_information_job_object)(
                    self.handle,
                    JOB_OBJECT_BASIC_PROCESS_ID_LIST,
                    (&mut stack_buf as *mut JobObjectBasicProcessIdListStack).cast::<c_void>(),
                    core::mem::size_of::<JobObjectBasicProcessIdListStack>() as u32,
                    &mut returned_len,
                )
            };

            if ok != 0 {
                let count = (stack_buf.number_of_process_ids_in_list as usize)
                    .min(stack_buf.process_id_list.len());
                let mut pids: Vec<u32> = stack_buf.process_id_list[..count]
                    .iter()
                    .filter_map(|&raw| u32::try_from(raw).ok().filter(|&p| p != 0))
                    .collect();
                if (stack_buf.number_of_assigned_processes as usize) <= count {
                    return pids;
                }
                // Fallback heap buffer if a tree spawns more than 128 concurrent child processes.
                let needed = (stack_buf.number_of_assigned_processes as usize).min(2048);
                let header_slots = if core::mem::size_of::<usize>() == 8 { 1 } else { 2 };
                let mut heap_buf = vec![0usize; header_slots + needed];
                let ok_large = unsafe {
                    (procs.query_information_job_object)(
                        self.handle,
                        JOB_OBJECT_BASIC_PROCESS_ID_LIST,
                        heap_buf.as_mut_ptr().cast::<c_void>(),
                        (heap_buf.len() * core::mem::size_of::<usize>()) as u32,
                        &mut returned_len,
                    )
                };
                if ok_large != 0 {
                    let counts: [u32; 2] =
                        unsafe { core::ptr::read(heap_buf.as_ptr().cast::<[u32; 2]>()) };
                    let large_count = (counts[1] as usize).min(needed);
                    pids = heap_buf[header_slots..header_slots + large_count]
                        .iter()
                        .filter_map(|&raw| u32::try_from(raw).ok().filter(|&p| p != 0))
                        .collect();
                }
                return pids;
            }
            Vec::new()
        }
    }

    /// Creates a Win32 Job Object and binds `root_pid` so all descendant processes are tracked.
    pub fn create_process_tree_job(root_pid: u32) -> Result<ProcessTreeJob, String> {
        let job = ProcessTreeJob::create()?;
        job.assign_pid(root_pid)?;
        Ok(job)
    }

    /// Suspends all processes in a tracked process tree via `NtSuspendProcess` and trims RAM.
    pub fn suspend_process_tree(pids: &[u32]) -> Result<f64, String> {
        let mut total_trimmed_mb = 0.0_f64;
        let mut succeeded = 0usize;
        let mut last_err = None;
        for &pid in pids {
            if pid == 0 {
                continue;
            }
            match suspend_external_pid(pid) {
                Ok(mb) => {
                    total_trimmed_mb += mb;
                    succeeded += 1;
                }
                Err(err) => {
                    last_err = Some(err);
                }
            }
        }
        if succeeded > 0 {
            Ok(total_trimmed_mb)
        } else {
            Err(last_err.unwrap_or_else(|| "No valid PIDs in process tree to suspend".to_string()))
        }
    }

    /// Creates an NTFS hard link at `new_link` pointing to `existing_file` via Win32 `CreateHardLinkW`.
    pub fn create_hard_link(
        new_link: &std::path::Path,
        existing_file: &std::path::Path,
    ) -> std::io::Result<()> {
        let link_fn = (*CREATE_HARD_LINK_W.get_or_init(|| unsafe {
            let hmod = GetModuleHandleA(PCSTR::from_raw(c"kernel32.dll".as_ptr().cast())).ok()?;
            let proc = GetProcAddress(hmod, PCSTR::from_raw(c"CreateHardLinkW".as_ptr().cast()))?;
            Some(std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                CreateHardLinkWFn,
            >(proc))
        }))
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Unsupported, "CreateHardLinkW unavailable"))?;

        let new_w: Vec<u16> = new_link
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(core::iter::once(0))
            .collect();
        let existing_w: Vec<u16> = existing_file
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(core::iter::once(0))
            .collect();

        let ok = unsafe { link_fn(new_w.as_ptr(), existing_w.as_ptr(), core::ptr::null()) };
        if ok == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// Resumes all suspended processes in a tracked process tree via `NtResumeProcess`.
    pub fn resume_process_tree(pids: &[u32]) -> Result<(), String> {
        let mut succeeded = 0usize;
        let mut last_err = None;
        for &pid in pids {
            if pid == 0 {
                continue;
            }
            match resume_external_pid(pid) {
                Ok(()) => succeeded += 1,
                Err(err) => last_err = Some(err),
            }
        }
        if succeeded > 0 {
            Ok(())
        } else {
            Err(last_err.unwrap_or_else(|| "No valid PIDs in process tree to resume".to_string()))
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod ext_ffi {
    pub struct ProcessTreeJob;

    impl ProcessTreeJob {
        pub fn create() -> Result<Self, String> {
            Err("Win32 Job Objects are only supported on Windows".to_string())
        }
        pub fn assign_pid(&self, _pid: u32) -> Result<(), String> {
            Err("Win32 Job Objects are only supported on Windows".to_string())
        }
        pub fn active_pids(&self) -> Vec<u32> {
            Vec::new()
        }
    }

    pub fn create_process_tree_job(_root_pid: u32) -> Result<ProcessTreeJob, String> {
        Err("Win32 Job Objects are only supported on Windows".to_string())
    }

    pub fn suspend_process_tree(_pids: &[u32]) -> Result<f64, String> {
        Err("Process suspend is only supported on Windows".to_string())
    }

    pub fn resume_process_tree(_pids: &[u32]) -> Result<(), String> {
        Err("Process resume is only supported on Windows".to_string())
    }

    pub fn suspend_external_pid(_pid: u32) -> Result<f64, String> {
        Err("Process suspend is only supported on Windows".to_string())
    }
    pub fn resume_external_pid(_pid: u32) -> Result<(), String> {
        Err("Process resume is only supported on Windows".to_string())
    }
    pub fn poll_xinput_state() -> super::XInputPadState {
        super::XInputPadState::default()
    }
    pub fn is_vk_down(_vk: i32) -> bool {
        false
    }
    pub fn reg_read_string(_use_hklm: bool, _subkey: &str, _value_name: &str) -> Option<String> {
        None
    }
    pub fn reg_enum_subkeys(_use_hklm: bool, _subkey: &str) -> Vec<String> {
        Vec::new()
    }
    pub fn find_pid_by_exe_name(_exe_name: &str) -> Option<u32> {
        None
    }
    pub fn create_hard_link(
        new_link: &std::path::Path,
        existing_file: &std::path::Path,
    ) -> std::io::Result<()> {
        std::fs::hard_link(existing_file, new_link)
    }
}

/// Snapshot of the active XInput gamepad state (zero allocations when polling).
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct XInputPadState {
    pub connected: bool,
    pub packet_number: u32,
    pub dpad_up: bool,
    pub dpad_down: bool,
    pub dpad_left: bool,
    pub dpad_right: bool,
    pub a: bool,
    pub b: bool,
    pub x: bool,
    pub y: bool,
    #[serde(default)]
    pub guide: bool,
    pub lb: bool,
    pub rb: bool,
    pub start: bool,
    pub back: bool,
    pub thumb_lx: i16,
    pub thumb_ly: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotkeySpec {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key_vk: i32,
}

/// Parses hotkey notation such as `"Pause"`, `"Ctrl+Alt+P"`, or `"F10"`.
pub fn parse_hotkey_spec(spec: &str) -> Option<HotkeySpec> {
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut key_vk: Option<i32> = None;

    for raw_part in spec.split('+') {
        let part = raw_part.trim().to_ascii_uppercase();
        if part.is_empty() {
            continue;
        }
        match part.as_str() {
            "CTRL" | "CONTROL" => ctrl = true,
            "ALT" | "MENU" => alt = true,
            "SHIFT" => shift = true,
            "PAUSE" | "BREAK" => key_vk = Some(0x13),
            "SCROLL" | "SCROLLLOCK" => key_vk = Some(0x91),
            "INSERT" => key_vk = Some(0x2D),
            "HOME" => key_vk = Some(0x24),
            "END" => key_vk = Some(0x23),
            "F1" => key_vk = Some(0x70),
            "F2" => key_vk = Some(0x71),
            "F3" => key_vk = Some(0x72),
            "F4" => key_vk = Some(0x73),
            "F5" => key_vk = Some(0x74),
            "F6" => key_vk = Some(0x75),
            "F7" => key_vk = Some(0x76),
            "F8" => key_vk = Some(0x77),
            "F9" => key_vk = Some(0x78),
            "F10" => key_vk = Some(0x79),
            "F11" => key_vk = Some(0x7A),
            "F12" => key_vk = Some(0x7B),
            single if single.len() == 1 => {
                let b = single.as_bytes()[0];
                if b.is_ascii_alphanumeric() {
                    key_vk = Some(b as i32);
                }
            }
            _ => {}
        }
    }

    key_vk.map(|vk| HotkeySpec {
        ctrl,
        alt,
        shift,
        key_vk: vk,
    })
}

/// Returns true only while the exact modifier + key combination in `spec` is held down.
pub fn is_hotkey_pressed(spec: &HotkeySpec) -> bool {
    const VK_SHIFT: i32 = 0x10;
    const VK_CONTROL: i32 = 0x11;
    const VK_MENU: i32 = 0x12;

    if !ext_ffi::is_vk_down(spec.key_vk) {
        return false;
    }
    let ctrl_down = ext_ffi::is_vk_down(VK_CONTROL);
    let alt_down = ext_ffi::is_vk_down(VK_MENU);
    let shift_down = ext_ffi::is_vk_down(VK_SHIFT);
    ctrl_down == spec.ctrl && alt_down == spec.alt && shift_down == spec.shift
}

pub use ext_ffi::{
    create_hard_link, create_process_tree_job, find_pid_by_exe_name, is_vk_down, poll_xinput_state,
    reg_enum_subkeys, reg_read_string, resume_external_pid, resume_process_tree,
    suspend_external_pid, suspend_process_tree, ProcessTreeJob,
};

/// Promotes every process in a tracked game tree to `ABOVE_NORMAL_PRIORITY_CLASS`.
#[cfg(feature = "eco-qos")]
pub fn boost_process_tree(pids: &[u32]) {
    for &pid in pids {
        if pid != 0 {
            let _ = boost_external_pid(pid);
        }
    }
}

#[cfg(all(test, target_os = "windows"))]
mod job_object_tests {
    use super::ProcessTreeJob;

    #[test]
    fn win32_job_object_tracks_spawned_child_tree() {
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "timeout /T 2 /NOBREAK >nul"])
            .spawn()
            .expect("spawn short-lived cmd helper");
        let pid = child.id();

        let job = ProcessTreeJob::create().expect("CreateJobObjectW should succeed");
        match job.assign_pid(pid) {
            Ok(()) => {
                let active = job.active_pids();
                assert!(
                    active.contains(&pid),
                    "expected assigned pid {pid} in active job pids {active:?}"
                );
            }
            Err(err) => {
                // Outer CI/container jobs with disallowed nested jobs still must not panic.
                assert!(!err.is_empty());
            }
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[cfg(feature = "win32-trim")]
pub use imp::{
    get_current_working_set_mb, get_external_working_set_mb, trim_current_working_set,
    trim_external_pid,
};

#[cfg(feature = "eco-qos")]
pub use imp::{
    boost_external_pid, disable_realtime_input_timer, enable_eco_qos_mode,
    enable_idle_eco_qos_mode, enable_realtime_input_mode, jail_external_pid_eco_qos,
    restore_normal_priority_mode, set_current_thread_high_priority,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hotkey_spec_supports_single_and_modifier_combos() {
        assert_eq!(
            parse_hotkey_spec("Pause"),
            Some(HotkeySpec {
                ctrl: false,
                alt: false,
                shift: false,
                key_vk: 0x13,
            })
        );
        assert_eq!(
            parse_hotkey_spec("Ctrl+Alt+P"),
            Some(HotkeySpec {
                ctrl: true,
                alt: true,
                shift: false,
                key_vk: b'P' as i32,
            })
        );
        assert_eq!(parse_hotkey_spec(""), None);
    }

    #[test]
    fn xinput_and_reg_probe_never_panic() {
        let pad = poll_xinput_state();
        assert!(!pad.connected || pad.packet_number < u32::MAX);
        assert!(reg_read_string(false, "Software\\DukeSystems__NonExistentTestKey__", "X").is_none());
    }
}
