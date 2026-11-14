//! Lock-free, cache-line-aligned Windows Shared Memory Ring Buffer (`Local\duke-suite-shm-v1`).
//!
//! Eliminates high-frequency JSON pipe chatter for telemetry ticks and lifecycle pulses
//! with fixed-size 64-byte `#[repr(C, align(64))]` frames (~40 ns cross-app IPC),
//! while `\\.\pipe\duke-suite` handles variable-length string/control messages.

use std::mem::MaybeUninit;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering, fence};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DUKE_SHM_NAME: &str = r"Local\duke-suite-shm-v1";
pub const SHM_RING_CAPACITY: usize = 1024;
const SHM_MAGIC: u64 = 0x4455_4B45_5348_4D31; // "DUKESHM1"
const SHM_VERSION: u32 = 1;

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShmEventType {
    None = 0,
    GameLaunched = 1,
    GameExited = 2,
    GameSuspended = 3,
    RamPurge = 4,
    TelemetryTick = 5,
    GameResumed = 6,
    ReplaySaveRequested = 7,
}

impl ShmEventType {
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::GameLaunched,
            2 => Self::GameExited,
            3 => Self::GameSuspended,
            4 => Self::RamPurge,
            5 => Self::TelemetryTick,
            6 => Self::GameResumed,
            7 => Self::ReplaySaveRequested,
            _ => Self::None,
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShmSourceApp {
    Unknown = 0,
    DukeLauncher = 1,
    DukeSystems = 2,
    DukeMonitor = 3,
    DukeCapture = 4,
    DukeInput = 5,
}

impl ShmSourceApp {
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::DukeLauncher,
            2 => Self::DukeSystems,
            3 => Self::DukeMonitor,
            4 => Self::DukeCapture,
            5 => Self::DukeInput,
            _ => Self::Unknown,
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::DukeLauncher => "Duke Launcher",
            Self::DukeSystems => "Duke Systems",
            Self::DukeMonitor => "Duke Monitor",
            Self::DukeCapture => "Duke Capture",
            Self::DukeInput => "Duke Input",
        }
    }
}

/// Packs GPU temperature (°C) and GPU utilization (%) into a single `u64` (`payload_a`)
/// for lock-free `ShmEventType::TelemetryTick` frames.
#[inline]
pub const fn pack_gpu_telemetry(gpu_temp_c: Option<u32>, gpu_load_pct: Option<u32>) -> u64 {
    let temp = match gpu_temp_c {
        Some(t) => t,
        None => u32::MAX,
    };
    let load = match gpu_load_pct {
        Some(l) => l,
        None => u32::MAX,
    };
    ((temp as u64) << 32) | (load as u64)
}

/// Unpacks `(gpu_temp_c, gpu_load_pct)` from a `TelemetryTick` frame's `payload_a`.
#[inline]
pub const fn unpack_gpu_telemetry(payload_a: u64) -> (Option<u32>, Option<u32>) {
    let raw_temp = (payload_a >> 32) as u32;
    let raw_load = (payload_a & 0xFFFF_FFFF) as u32;
    let temp = if raw_temp == u32::MAX { None } else { Some(raw_temp) };
    let load = if raw_load == u32::MAX { None } else { Some(raw_load) };
    (temp, load)
}

/// Fixed 64-byte cache-line-aligned IPC event frame (`#[repr(C, align(64))]`).
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShmEventFrame {
    pub event_type: u32,
    pub source_app: u32,
    pub pid: u32,
    pub timestamp_us: u64,
    pub payload_a: u64,
    pub payload_b: u64,
    pub reserved: [u8; 24],
}

const _: () = assert!(std::mem::size_of::<ShmEventFrame>() == 64);

impl Default for ShmEventFrame {
    fn default() -> Self {
        Self::new(
            ShmEventType::None,
            ShmSourceApp::Unknown,
            0,
            0,
            0,
            0,
            [0u8; 24],
        )
    }
}

impl ShmEventFrame {
    /// Constructs a 64-byte event frame with deterministic zeroed alignment padding.
    pub fn new(
        event_type: ShmEventType,
        source_app: ShmSourceApp,
        pid: u32,
        timestamp_us: u64,
        payload_a: u64,
        payload_b: u64,
        reserved: [u8; 24],
    ) -> Self {
        let mut uninit = MaybeUninit::<Self>::zeroed();
        // SAFETY: All fields are Plain Old Data integers/bytes and initialized before `assume_init`.
        unsafe {
            let ptr = uninit.as_mut_ptr();
            std::ptr::addr_of_mut!((*ptr).event_type).write(event_type as u32);
            std::ptr::addr_of_mut!((*ptr).source_app).write(source_app as u32);
            std::ptr::addr_of_mut!((*ptr).pid).write(pid);
            std::ptr::addr_of_mut!((*ptr).timestamp_us).write(timestamp_us);
            std::ptr::addr_of_mut!((*ptr).payload_a).write(payload_a);
            std::ptr::addr_of_mut!((*ptr).payload_b).write(payload_b);
            std::ptr::addr_of_mut!((*ptr).reserved).write(reserved);
            uninit.assume_init()
        }
    }

    /// Creates a timestamped frame embedding up to 24 bytes of UTF-8 tag text in `reserved`.
    pub fn with_tag(
        event_type: ShmEventType,
        source_app: ShmSourceApp,
        pid: u32,
        payload_a: u64,
        payload_b: u64,
        tag: &str,
    ) -> Self {
        let mut reserved = [0u8; 24];
        let mut copy_len = tag.len().min(reserved.len());
        while copy_len > 0 && !tag.is_char_boundary(copy_len) {
            copy_len -= 1;
        }
        reserved[..copy_len].copy_from_slice(&tag.as_bytes()[..copy_len]);
        Self::new(
            event_type,
            source_app,
            pid,
            now_unix_micros(),
            payload_a,
            payload_b,
            reserved,
        )
    }

    pub fn kind(&self) -> ShmEventType {
        ShmEventType::from_u32(self.event_type)
    }

    pub fn app(&self) -> ShmSourceApp {
        ShmSourceApp::from_u32(self.source_app)
    }

    pub fn tag_str(&self) -> &str {
        let end = self
            .reserved
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.reserved.len());
        std::str::from_utf8(&self.reserved[..end]).unwrap_or("")
    }
}

#[repr(C, align(64))]
struct CachePaddedAtomicU64 {
    value: AtomicU64,
    _pad: [u8; 56],
}

const _: () = assert!(std::mem::size_of::<CachePaddedAtomicU64>() == 64);

#[repr(C, align(64))]
struct ShmHeaderMeta {
    magic: AtomicU64,
    version: u32,
    capacity: u32,
    creator_pid: AtomicU32,
    app_pids: [AtomicU32; 6],
    _pad: [u8; 20],
}

const _: () = assert!(std::mem::size_of::<ShmHeaderMeta>() == 64);

/// Shared memory layout mapped identically across processes.
#[repr(C, align(64))]
struct ShmRingRegion {
    meta: ShmHeaderMeta,
    claim_seq: CachePaddedAtomicU64,
    write_seq: CachePaddedAtomicU64,
    read_seq: CachePaddedAtomicU64,
    slot_seq: [AtomicU64; SHM_RING_CAPACITY],
    frames: [ShmEventFrame; SHM_RING_CAPACITY],
}

pub fn now_unix_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

/// Cross-process handle to the lock-free shared memory ring buffer.
pub struct DukeShmBus {
    region: NonNull<ShmRingRegion>,
    #[cfg(target_os = "windows")]
    mapping_handle: windows::Win32::Foundation::HANDLE,
    #[cfg(not(target_os = "windows"))]
    _owned_backing: Option<Box<ShmRingRegion>>,
    local_read_seq: u64,
    lagged_count: u64,
    registered_slot: AtomicU32,
}

unsafe impl Send for DukeShmBus {}
unsafe impl Sync for DukeShmBus {}

impl DukeShmBus {
    /// Opens or creates the global Suite shared memory bus (`Local\duke-suite-shm-v1`).
    pub fn open_or_create() -> Result<Self, String> {
        Self::open_or_create_named(DUKE_SHM_NAME)
    }

    /// Opens or creates a named shared memory ring buffer mapping (primarily for test isolation).
    #[cfg(target_os = "windows")]
    pub fn open_or_create_named(mapping_name: &str) -> Result<Self, String> {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, INVALID_HANDLE_VALUE};
        use windows::Win32::System::Memory::{
            CreateFileMappingW, FILE_MAP_ALL_ACCESS, MapViewOfFile, PAGE_READWRITE,
        };
        use windows::core::PCWSTR;

        let wide_name: Vec<u16> = OsStr::new(mapping_name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let region_bytes = std::mem::size_of::<ShmRingRegion>();
        let high = ((region_bytes as u64) >> 32) as u32;
        let low = (region_bytes as u64 & 0xFFFF_FFFF) as u32;

        // SAFETY: Win32 CreateFileMappingW with PAGE_READWRITE over paging file (INVALID_HANDLE_VALUE).
        let handle = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                None,
                PAGE_READWRITE,
                high,
                low,
                PCWSTR(wide_name.as_ptr()),
            )
        }
        .map_err(|e| format!("CreateFileMappingW('{}') failed: {}", mapping_name, e))?;

        // SAFETY: Immediately reading Win32 GetLastError after CreateFileMappingW.
        let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;

        // SAFETY: Mapping the full region into our process address space.
        let mapped = unsafe { MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, region_bytes) };
        if mapped.Value.is_null() {
            // SAFETY: Cleaning up valid handle on failure.
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err(format!("MapViewOfFile('{}') returned null.", mapping_name));
        }

        let ptr = mapped.Value as *mut ShmRingRegion;
        let Some(region_nn) = NonNull::new(ptr) else {
            // SAFETY: Handle cleanup.
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err("Mapped SHM pointer was null.".to_string());
        };

        if !already_exists {
            // SAFETY: We are the creator before magic publication; zero-initialize the mapped page range.
            unsafe {
                std::ptr::write_bytes(ptr as *mut u8, 0, region_bytes);
                (*ptr).meta.version = SHM_VERSION;
                (*ptr).meta.capacity = SHM_RING_CAPACITY as u32;
                (*ptr)
                    .meta
                    .creator_pid
                    .store(std::process::id(), Ordering::Relaxed);
                (*ptr)
                    .meta
                    .magic
                    .store(SHM_MAGIC, Ordering::Release);
            }
        } else {
            // Wait briefly if another process created the section and is finishing header zeroing.
            let mut ready = false;
            for _ in 0..200 {
                // SAFETY: Reading atomic magic from valid mapped region.
                let magic = unsafe { (*ptr).meta.magic.load(Ordering::Acquire) };
                if magic == SHM_MAGIC {
                    ready = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_micros(50));
            }
            if !ready {
                // Fallback initialize if creator died mid-init.
                // SAFETY: Re-initializing unsealed header.
                unsafe {
                    (*ptr).meta.version = SHM_VERSION;
                    (*ptr).meta.capacity = SHM_RING_CAPACITY as u32;
                    (*ptr).meta.magic.store(SHM_MAGIC, Ordering::Release);
                }
            }
        }

        // Start SPMC consumer cursor at current write head so newly launched listeners receive live pulses.
        // Call `reset_cursor_to_zero()` if historical replay from ring tail is desired.
        // SAFETY: Region magic has been initialized/verified above.
        let current_head = unsafe { (*ptr).write_seq.value.load(Ordering::Acquire) };
        let initial_cursor = current_head.saturating_sub(SHM_RING_CAPACITY as u64);

        Ok(Self {
            region: region_nn,
            mapping_handle: handle,
            local_read_seq: if already_exists {
                current_head
            } else {
                initial_cursor
            },
            lagged_count: 0,
            registered_slot: AtomicU32::new(0),
        })
    }

    #[cfg(not(target_os = "windows"))]
    pub fn open_or_create_named(_mapping_name: &str) -> Result<Self, String> {
        let mut region = Box::new(unsafe { MaybeUninit::<ShmRingRegion>::zeroed().assume_init() });
        region.meta.version = SHM_VERSION;
        region.meta.capacity = SHM_RING_CAPACITY as u32;
        region
            .meta
            .creator_pid
            .store(std::process::id(), Ordering::Relaxed);
        region.meta.magic.store(SHM_MAGIC, Ordering::Release);
        let ptr = NonNull::from(region.as_mut());
        Ok(Self {
            region: ptr,
            _owned_backing: Some(region),
            local_read_seq: 0,
            lagged_count: 0,
            registered_slot: AtomicU32::new(0),
        })
    }

    /// Registers the current process under `app` in the shared header so Suite Hub can report active SHM apps.
    pub fn register_app(&self, app: ShmSourceApp) {
        let slot = app as u32;
        if (1..=5).contains(&slot) {
            self.region().meta.app_pids[slot as usize]
                .store(std::process::id(), Ordering::Release);
            self.registered_slot.store(slot, Ordering::Release);
        }
    }

    /// Clears this handle's registered process slot if it still points to the current PID.
    pub fn unregister_app(&self) {
        let slot = self.registered_slot.swap(0, Ordering::AcqRel);
        if (1..=5).contains(&slot) {
            let _ = self.region().meta.app_pids[slot as usize].compare_exchange(
                std::process::id(),
                0,
                Ordering::AcqRel,
                Ordering::Relaxed,
            );
        }
    }

    /// Returns all Suite applications currently registered and alive on this SHM ring buffer.
    /// Dead PIDs left behind by hard process termination are self-healed to `0`.
    pub fn active_apps(&self) -> Vec<ShmSourceApp> {
        let meta = &self.region().meta;
        let mut active = Vec::with_capacity(5);
        for slot in 1u32..=5u32 {
            let pid = meta.app_pids[slot as usize].load(Ordering::Acquire);
            if pid == 0 {
                continue;
            }
            if is_process_alive(pid) {
                active.push(ShmSourceApp::from_u32(slot));
            } else {
                let _ = meta.app_pids[slot as usize].compare_exchange(
                    pid,
                    0,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                );
            }
        }
        active
    }

    #[inline]
    fn region(&self) -> &ShmRingRegion {
        // SAFETY: `self.region` stays mapped until `Drop`.
        unsafe { self.region.as_ref() }
    }

    /// Rewinds this consumer's local cursor to the oldest retained sequence in the ring.
    pub fn seek_oldest(&mut self) {
        let head = self.region().write_seq.value.load(Ordering::Acquire);
        self.local_read_seq = head.saturating_sub(SHM_RING_CAPACITY as u64);
    }

    /// Explicitly sets local consumer sequence (useful when opening a second view at index 0).
    pub fn seek_sequence(&mut self, seq: u64) {
        self.local_read_seq = seq;
    }

    pub fn write_cursor(&self) -> u64 {
        self.region().write_seq.value.load(Ordering::Acquire)
    }

    pub fn shared_read_cursor(&self) -> u64 {
        self.region().read_seq.value.load(Ordering::Acquire)
    }

    pub fn local_cursor(&self) -> u64 {
        self.local_read_seq
    }

    pub fn lagged_events(&self) -> u64 {
        self.lagged_count
    }

    /// Lock-free publish of a 64-byte `ShmEventFrame` into the ring buffer. Returns assigned sequence.
    pub fn publish_frame(&self, frame: ShmEventFrame) -> u64 {
        let region = self.region();
        let seq = region.claim_seq.value.fetch_add(1, Ordering::Relaxed);
        let slot_idx = (seq as usize) & (SHM_RING_CAPACITY - 1);
        let writing_stamp = seq.wrapping_mul(2).wrapping_add(1);
        let committed_stamp = seq.wrapping_mul(2).wrapping_add(2);

        region.slot_seq[slot_idx].store(writing_stamp, Ordering::Release);
        // SAFETY: `slot_idx < SHM_RING_CAPACITY`; this producer holds unique ownership of `seq`.
        unsafe {
            let dst = region.frames.as_ptr().add(slot_idx) as *mut ShmEventFrame;
            std::ptr::write_volatile(dst, frame);
        }
        region.slot_seq[slot_idx].store(committed_stamp, Ordering::Release);
        region
            .write_seq
            .value
            .fetch_max(seq.saturating_add(1), Ordering::Release);
        seq
    }

    /// Convenience helper constructing and publishing a tagged 64-byte frame.
    pub fn publish_event(
        &self,
        event_type: ShmEventType,
        source_app: ShmSourceApp,
        pid: u32,
        payload_a: u64,
        payload_b: u64,
        tag: &str,
    ) -> u64 {
        let frame = ShmEventFrame::with_tag(event_type, source_app, pid, payload_a, payload_b, tag);
        self.publish_frame(frame)
    }

    /// Non-blocking SPMC consumer poll: each consumer tracks its own `local_read_seq` without contention.
    pub fn try_recv(&mut self) -> Option<ShmEventFrame> {
        let region_ptr = self.region.as_ptr();
        loop {
            // SAFETY: `region_ptr` points to the active mapped SHM region for the lifetime of `&mut self`.
            let region = unsafe { &*region_ptr };
            let head = region.write_seq.value.load(Ordering::Acquire);
            if self.local_read_seq >= head {
                return None;
            }
            let min_retained = head.saturating_sub(SHM_RING_CAPACITY as u64);
            if self.local_read_seq < min_retained {
                self.lagged_count = self
                    .lagged_count
                    .saturating_add(min_retained - self.local_read_seq);
                self.local_read_seq = min_retained;
            }

            let seq = self.local_read_seq;
            let slot_idx = (seq as usize) & (SHM_RING_CAPACITY - 1);
            let committed_stamp = seq.wrapping_mul(2).wrapping_add(2);

            let mut stamp = region.slot_seq[slot_idx].load(Ordering::Acquire);
            if stamp < committed_stamp {
                // Multi-producer hole: a concurrent producer claimed `seq` and is finishing its 64B copy.
                let mut spins = 0u32;
                while stamp < committed_stamp && spins < 128 {
                    std::hint::spin_loop();
                    stamp = region.slot_seq[slot_idx].load(Ordering::Acquire);
                    spins += 1;
                }
                if stamp < committed_stamp {
                    return None;
                }
            }
            if stamp > committed_stamp {
                // Writer wrapped past this slot before we read it.
                self.lagged_count = self.lagged_count.saturating_add(1);
                self.local_read_seq = seq.saturating_add(1);
                continue;
            }

            // SAFETY: `slot_idx < SHM_RING_CAPACITY` within mapped memory.
            let frame = unsafe { std::ptr::read_volatile(&region.frames[slot_idx]) };
            fence(Ordering::Acquire);
            if region.slot_seq[slot_idx].load(Ordering::Acquire) != committed_stamp {
                // Overwritten during read; skip wrapped slot cleanly.
                self.lagged_count = self.lagged_count.saturating_add(1);
                self.local_read_seq = seq.saturating_add(1);
                continue;
            }

            self.local_read_seq = seq.saturating_add(1);
            return Some(frame);
        }
    }

    /// Lock-free SPSC consumer pop using the shared `read_seq` (`Acquire`/`Release`) cursor.
    pub fn try_pop_spsc(&self) -> Option<ShmEventFrame> {
        let region = self.region();
        loop {
            let head = region.write_seq.value.load(Ordering::Acquire);
            let mut tail = region.read_seq.value.load(Ordering::Acquire);
            if tail >= head {
                return None;
            }
            let min_retained = head.saturating_sub(SHM_RING_CAPACITY as u64);
            if tail < min_retained {
                tail = min_retained;
            }

            let slot_idx = (tail as usize) & (SHM_RING_CAPACITY - 1);
            let committed_stamp = tail.wrapping_mul(2).wrapping_add(2);
            let stamp = region.slot_seq[slot_idx].load(Ordering::Acquire);
            if stamp < committed_stamp {
                return None;
            }
            if stamp > committed_stamp {
                let _ = region.read_seq.value.compare_exchange(
                    tail,
                    tail.saturating_add(1),
                    Ordering::Release,
                    Ordering::Relaxed,
                );
                continue;
            }

            // SAFETY: `slot_idx < SHM_RING_CAPACITY`.
            let frame = unsafe { std::ptr::read_volatile(&region.frames[slot_idx]) };
            fence(Ordering::Acquire);
            if region.slot_seq[slot_idx].load(Ordering::Acquire) != committed_stamp {
                continue;
            }

            if region
                .read_seq
                .value
                .compare_exchange(
                    tail,
                    tail.saturating_add(1),
                    Ordering::Release,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                return Some(frame);
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn is_process_alive(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    if pid <= 4 {
        return false;
    }
    use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    const STILL_ACTIVE_CODE: u32 = 259;

    // SAFETY: Standard Win32 OpenProcess + GetExitCodeProcess handle lifecycle check.
    unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(handle) => {
                let mut exit_code: u32 = 0;
                let ok = GetExitCodeProcess(handle, &mut exit_code).is_ok();
                let _ = CloseHandle(handle);
                ok && exit_code == STILL_ACTIVE_CODE
            }
            Err(err) => err.code() == ERROR_ACCESS_DENIED.to_hresult(),
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn is_process_alive(pid: u32) -> bool {
    pid == std::process::id()
}

impl Drop for DukeShmBus {
    fn drop(&mut self) {
        self.unregister_app();
        #[cfg(target_os = "windows")]
        // SAFETY: `self.region` and `self.mapping_handle` are valid until Drop.
        unsafe {
            use windows::Win32::Foundation::CloseHandle;
            use windows::Win32::System::Memory::{MEMORY_MAPPED_VIEW_ADDRESS, UnmapViewOfFile};
            let _ = UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.region.as_ptr() as *mut std::ffi::c_void,
            });
            let _ = CloseHandle(self.mapping_handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_is_exact_64_bytes_and_tag_roundtrips() {
        assert_eq!(std::mem::size_of::<ShmEventFrame>(), 64);
        assert_eq!(std::mem::align_of::<ShmEventFrame>(), 64);

        let frame = ShmEventFrame::with_tag(
            ShmEventType::GameLaunched,
            ShmSourceApp::DukeLauncher,
            1337,
            0xDEAD_BEEF,
            42,
            "eldenring.exe",
        );
        assert_eq!(frame.kind(), ShmEventType::GameLaunched);
        assert_eq!(frame.app(), ShmSourceApp::DukeLauncher);
        assert_eq!(frame.pid, 1337);
        assert_eq!(frame.payload_a, 0xDEAD_BEEF);
        assert_eq!(frame.payload_b, 42);
        assert_eq!(frame.tag_str(), "eldenring.exe");
    }

    #[test]
    fn shm_ring_buffer_spmc_spsc_and_wraparound_work() {
        let name = format!(r"Local\duke-suite-shm-unit-{}", std::process::id());
        let producer = DukeShmBus::open_or_create_named(&name).unwrap();
        let mut consumer_a = DukeShmBus::open_or_create_named(&name).unwrap();
        let mut consumer_b = DukeShmBus::open_or_create_named(&name).unwrap();

        consumer_a.seek_sequence(0);
        consumer_b.seek_sequence(0);

        producer.publish_event(
            ShmEventType::GameLaunched,
            ShmSourceApp::DukeLauncher,
            1001,
            1,
            2,
            "hades2.exe",
        );
        producer.publish_event(
            ShmEventType::RamPurge,
            ShmSourceApp::DukeSystems,
            1002,
            512,
            64,
            "idle-trim",
        );

        let ev_a1 = consumer_a.try_recv().unwrap();
        let ev_a2 = consumer_a.try_recv().unwrap();
        assert!(consumer_a.try_recv().is_none());
        assert_eq!(ev_a1.kind(), ShmEventType::GameLaunched);
        assert_eq!(ev_a2.kind(), ShmEventType::RamPurge);

        // Second independent consumer also observes both events via SPMC cursor
        let ev_b1 = consumer_b.try_recv().unwrap();
        let ev_b2 = consumer_b.try_recv().unwrap();
        assert_eq!(ev_b1.tag_str(), "hades2.exe");
        assert_eq!(ev_b2.payload_a, 512);

        // SPSC shared cursor also pops from 0 independently
        let spsc1 = producer.try_pop_spsc().unwrap();
        let spsc2 = producer.try_pop_spsc().unwrap();
        assert!(producer.try_pop_spsc().is_none());
        assert_eq!(spsc1.pid, 1001);
        assert_eq!(spsc2.pid, 1002);

        // Overwrite beyond ring capacity (1024) to verify wrap-around lag recovery
        let overflow_count = (SHM_RING_CAPACITY as u64) + 150;
        for idx in 0..overflow_count {
            producer.publish_event(
                ShmEventType::TelemetryTick,
                ShmSourceApp::DukeMonitor,
                2000,
                idx,
                idx * 2,
                "tick",
            );
        }

        let mut received = Vec::new();
        while let Some(ev) = consumer_a.try_recv() {
            received.push(ev.payload_a);
        }
        assert_eq!(received.len(), SHM_RING_CAPACITY);
        assert!(consumer_a.lagged_events() >= 150);
        assert_eq!(*received.last().unwrap(), overflow_count - 1);
    }

    #[test]
    fn gpu_telemetry_packing_and_app_registration_roundtrip() {
        let packed = pack_gpu_telemetry(Some(68), Some(94));
        assert_eq!(unpack_gpu_telemetry(packed), (Some(68), Some(94)));
        let packed_none = pack_gpu_telemetry(None, Some(15));
        assert_eq!(unpack_gpu_telemetry(packed_none), (None, Some(15)));

        let name = format!(r"Local\duke-suite-shm-apps-{}", std::process::id());
        let bus = DukeShmBus::open_or_create_named(&name).unwrap();
        assert!(bus.active_apps().is_empty());

        bus.register_app(ShmSourceApp::DukeMonitor);
        bus.register_app(ShmSourceApp::DukeSystems);
        let active = bus.active_apps();
        assert!(active.contains(&ShmSourceApp::DukeMonitor));
        assert!(active.contains(&ShmSourceApp::DukeSystems));

        bus.unregister_app();
        // `unregister_app` clears the last registered slot (`DukeSystems`)
        assert!(!bus.active_apps().contains(&ShmSourceApp::DukeSystems));
    }
}
