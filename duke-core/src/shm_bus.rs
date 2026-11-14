//! Lock-free, cache-line-aligned Windows Shared Memory Ring Buffer (`Local\duke-suite-shm-v1`).
//!
//! Eliminates high-frequency JSON pipe chatter for telemetry ticks and lifecycle pulses
//! with fixed-size 64-byte `#[repr(C, align(64))]` frames (~40 ns cross-app IPC),
//! while `\\.\pipe\duke-suite` handles variable-length string/control messages.

use std::mem::MaybeUninit;
use std::ptr::NonNull;
use std::sync::atomic::{fence, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DUKE_SHM_NAME: &str = r"Local\duke-suite-shm-v1";
pub const DUKE_AUDIO_TAP_SHM_NAME: &str = r"Local\duke-capture-audio-tap-v1";
pub const SHM_RING_CAPACITY: usize = 1024;
pub const AUDIO_TAP_RING_CAPACITY: usize = 32_768; // ~2.048s @ 16 kHz mono
const SHM_MAGIC: u64 = 0x4455_4B45_5348_4D31; // "DUKESHM1"
const AUDIO_TAP_MAGIC: u64 = 0x4455_4B45_4155_4431; // "DUKEAUD1"
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
    AchievementUnlocked = 8,
    ProfileSwitch = 9,
    CoOpInputP2 = 10,
    HighlightPeak = 11,
    VoiceMacro = 12,
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
            8 => Self::AchievementUnlocked,
            9 => Self::ProfileSwitch,
            10 => Self::CoOpInputP2,
            11 => Self::HighlightPeak,
            12 => Self::VoiceMacro,
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

/// Deterministic 64-bit FNV-1a hash used for compact executable/slug fingerprints in SHM frames.
#[inline]
pub const fn fnv1a_hash64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut idx = 0usize;
    while idx < bytes.len() {
        hash ^= bytes[idx] as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
        idx += 1;
    }
    hash
}

/// Packs `sandbox_enabled` (bit 0) and `platform_id` (`u16` at bits 16..31) into `GameLaunched::payload_b`.
/// Preserves bit 0 backwards-compatibility with legacy readers checking `payload_b & 1`.
#[inline]
pub const fn pack_game_launch_payload_b(sandbox_enabled: bool, platform_id: u16) -> u64 {
    pack_game_launch_payload_b_ext(sandbox_enabled, platform_id, 0)
}

/// Packs `sandbox_enabled` (bit 0), `platform_id` (`u16` bits 16..31), and `app_id` (`u32` bits 32..63)
/// into `GameLaunched::payload_b` / `GameExited::payload_b`.
#[inline]
pub const fn pack_game_launch_payload_b_ext(
    sandbox_enabled: bool,
    platform_id: u16,
    app_id: u32,
) -> u64 {
    ((app_id as u64) << 32) | ((platform_id as u64) << 16) | (sandbox_enabled as u64)
}

/// Unpacks whether sandbox mode was requested from `GameLaunched::payload_b`.
#[inline]
pub const fn unpack_game_launch_sandbox(payload_b: u64) -> bool {
    (payload_b & 1) != 0
}

/// Unpacks the canonical non-PC `platform_id` (`1..=24`) from `GameLaunched::payload_b`, returning `None` for PC (`0`).
#[inline]
pub const fn unpack_game_launch_platform_id(payload_b: u64) -> Option<u16> {
    let id = ((payload_b >> 16) & 0xFFFF) as u16;
    if id == 0 {
        None
    } else {
        Some(id)
    }
}

/// Unpacks `app_id` (`u32` bits 32..63) from `GameLaunched::payload_b` / `GameExited::payload_b`.
#[inline]
pub const fn unpack_game_launch_app_id(payload_b: u64) -> u32 {
    ((payload_b >> 32) & 0xFFFF_FFFF) as u32
}

/// Structured decoded payload for `ShmEventType::GameLaunched`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShmGameLaunchedPayload {
    pub pid: u32,
    pub app_id: u32,
    pub platform_id: u16,
    pub sandbox_enabled: bool,
    pub exe_hash: u64,
    pub tag: String,
}

/// Structured decoded payload for `ShmEventType::GameExited`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShmGameExitedPayload {
    pub pid: u32,
    pub app_id: u32,
    pub platform_id: u16,
    pub exe_hash: u64,
    pub tag: String,
}

/// Structured decoded payload for `ShmEventType::AchievementUnlocked`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShmAchievementUnlockedPayload {
    pub app_id: u32,
    pub title_slug_hash: u64,
    pub title_slug: String,
}

/// Structured decoded payload for `ShmEventType::ProfileSwitch`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShmProfileSwitchPayload {
    pub source_app: ShmSourceApp,
    pub app_id: u32,
    pub platform_id: u16,
    pub profile_hash: u64,
    pub profile_tag: String,
}

/// Compact zero-allocation virtual Player 2 gamepad state for `ShmEventType::CoOpInputP2`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ShmCoOpInputP2Payload {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub thumb_lx: i16,
    pub thumb_ly: i16,
    pub thumb_rx: i16,
    pub thumb_ry: i16,
    pub sequence: u32,
}

/// Canonical console display label for shared-memory `platform_id` (`1..=24`).
#[inline]
pub const fn console_label_for_platform_id(platform_id: u16) -> Option<&'static str> {
    match platform_id {
        1 => Some("NES"),
        2 => Some("SNES"),
        3 => Some("N64"),
        4 => Some("GameCube"),
        5 => Some("Wii"),
        6 => Some("Wii U"),
        7 => Some("Switch"),
        8 => Some("Game Boy"),
        9 => Some("Game Boy Color"),
        10 => Some("Game Boy Advance"),
        11 => Some("Nintendo DS"),
        12 => Some("Nintendo 3DS"),
        13 => Some("Virtual Boy"),
        14 => Some("PS1"),
        15 => Some("PS2"),
        16 => Some("PSP"),
        17 => Some("PS3"),
        18 => Some("Sega Genesis"),
        19 => Some("Sega Master System"),
        20 => Some("Sega Saturn"),
        21 => Some("Sega Dreamcast"),
        22 => Some("Atari 2600"),
        23 => Some("Arcade"),
        24 => Some("PC Engine"),
        _ => None,
    }
}

/// Returns `true` when the console natively uses motion/gyro controls (Wii, Wii U, Switch, 3DS, PS3).
#[inline]
pub const fn console_has_motion_gyro(platform_id: u16) -> bool {
    matches!(platform_id, 5 | 6 | 7 | 12 | 17)
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

    /// Builds a structured `ShmEventType::GameLaunched` frame carrying `(app_id, platform_id, exe_hash)`.
    pub fn game_launched(
        source_app: ShmSourceApp,
        pid: u32,
        app_id: u32,
        platform_id: u16,
        sandbox_enabled: bool,
        exe_hash: u64,
        tag: &str,
    ) -> Self {
        Self::with_tag(
            ShmEventType::GameLaunched,
            source_app,
            pid,
            exe_hash,
            pack_game_launch_payload_b_ext(sandbox_enabled, platform_id, app_id),
            tag,
        )
    }

    /// Builds a structured `ShmEventType::GameExited` frame carrying `(app_id, platform_id, exe_hash)`.
    pub fn game_exited(
        source_app: ShmSourceApp,
        pid: u32,
        app_id: u32,
        platform_id: u16,
        exe_hash: u64,
        tag: &str,
    ) -> Self {
        Self::with_tag(
            ShmEventType::GameExited,
            source_app,
            pid,
            exe_hash,
            pack_game_launch_payload_b_ext(false, platform_id, app_id),
            tag,
        )
    }

    /// Builds a structured `ShmEventType::AchievementUnlocked` frame carrying `{ app_id, title_slug }`.
    pub fn achievement_unlocked(
        source_app: ShmSourceApp,
        pid: u32,
        app_id: u32,
        title_slug: &str,
    ) -> Self {
        Self::with_tag(
            ShmEventType::AchievementUnlocked,
            source_app,
            pid,
            app_id as u64,
            fnv1a_hash64(title_slug.as_bytes()),
            title_slug,
        )
    }

    /// Builds a structured `ShmEventType::ProfileSwitch` frame notifying daemons of active profile changes.
    pub fn profile_switch(
        source_app: ShmSourceApp,
        pid: u32,
        app_id: u32,
        platform_id: u16,
        profile_tag: &str,
    ) -> Self {
        Self::with_tag(
            ShmEventType::ProfileSwitch,
            source_app,
            pid,
            fnv1a_hash64(profile_tag.as_bytes()),
            pack_game_launch_payload_b_ext(false, platform_id, app_id),
            profile_tag,
        )
    }

    /// Builds a zero-allocation Player 2 controller state frame for `ShmEventType::CoOpInputP2`.
    pub fn coop_input_p2(
        source_app: ShmSourceApp,
        pid: u32,
        state: ShmCoOpInputP2Payload,
    ) -> Self {
        let payload_a = ((state.sequence as u64) << 32)
            | ((state.right_trigger as u64) << 24)
            | ((state.left_trigger as u64) << 16)
            | (state.buttons as u64);
        let payload_b = (((state.thumb_ry as u16) as u64) << 48)
            | (((state.thumb_rx as u16) as u64) << 32)
            | (((state.thumb_ly as u16) as u64) << 16)
            | ((state.thumb_lx as u16) as u64);
        Self::with_tag(
            ShmEventType::CoOpInputP2,
            source_app,
            pid,
            payload_a,
            payload_b,
            "p2-pad",
        )
    }

    /// Builds a zero-allocation Wave 3 AI highlight peak trigger frame for `duke-capture`.
    pub fn highlight_peak(
        source_app: ShmSourceApp,
        pid: u32,
        intensity: u8,
        duration_secs: u8,
        tag: &str,
    ) -> Self {
        let packed = ((intensity as u64) << 8) | (duration_secs as u64);
        Self::with_tag(
            ShmEventType::HighlightPeak,
            source_app,
            pid,
            packed,
            fnv1a_hash64(tag.as_bytes()),
            tag,
        )
    }

    /// Builds a zero-allocation Wave 3 voice-macro trigger frame for `duke-input`.
    pub fn voice_macro(
        source_app: ShmSourceApp,
        pid: u32,
        vk_code: u16,
        hold_ms: u16,
        action_tag: &str,
    ) -> Self {
        let packed = ((vk_code as u64) << 16) | (hold_ms as u64);
        Self::with_tag(
            ShmEventType::VoiceMacro,
            source_app,
            pid,
            packed,
            fnv1a_hash64(action_tag.as_bytes()),
            action_tag,
        )
    }

    /// Decodes a `HighlightPeak` frame as `(intensity, duration_secs, tag)`.
    pub fn decode_highlight_peak(&self) -> Option<(u8, u8, String)> {
        if self.kind() != ShmEventType::HighlightPeak {
            return None;
        }
        let intensity = ((self.payload_a >> 8) & 0xFF) as u8;
        let duration_secs = (self.payload_a & 0xFF) as u8;
        Some((intensity, duration_secs, self.tag_str().to_string()))
    }

    /// Decodes a `VoiceMacro` frame as `(vk_code, hold_ms, action_tag)`.
    pub fn decode_voice_macro(&self) -> Option<(u16, u16, String)> {
        if self.kind() != ShmEventType::VoiceMacro {
            return None;
        }
        let vk_code = ((self.payload_a >> 16) & 0xFFFF) as u16;
        let hold_ms = (self.payload_a & 0xFFFF) as u16;
        Some((vk_code, hold_ms, self.tag_str().to_string()))
    }

    /// Decodes a `GameLaunched` frame into its structured payload.
    pub fn decode_game_launched(&self) -> Option<ShmGameLaunchedPayload> {
        if self.kind() != ShmEventType::GameLaunched {
            return None;
        }
        Some(ShmGameLaunchedPayload {
            pid: self.pid,
            app_id: unpack_game_launch_app_id(self.payload_b),
            platform_id: unpack_game_launch_platform_id(self.payload_b).unwrap_or(0),
            sandbox_enabled: unpack_game_launch_sandbox(self.payload_b),
            exe_hash: self.payload_a,
            tag: self.tag_str().to_string(),
        })
    }

    /// Decodes a `GameExited` frame into its structured payload.
    pub fn decode_game_exited(&self) -> Option<ShmGameExitedPayload> {
        if self.kind() != ShmEventType::GameExited {
            return None;
        }
        Some(ShmGameExitedPayload {
            pid: self.pid,
            app_id: unpack_game_launch_app_id(self.payload_b),
            platform_id: unpack_game_launch_platform_id(self.payload_b).unwrap_or(0),
            exe_hash: self.payload_a,
            tag: self.tag_str().to_string(),
        })
    }

    /// Decodes an `AchievementUnlocked` frame into `{ app_id, title_slug }`.
    pub fn decode_achievement_unlocked(&self) -> Option<ShmAchievementUnlockedPayload> {
        if self.kind() != ShmEventType::AchievementUnlocked {
            return None;
        }
        Some(ShmAchievementUnlockedPayload {
            app_id: (self.payload_a & 0xFFFF_FFFF) as u32,
            title_slug_hash: self.payload_b,
            title_slug: self.tag_str().to_string(),
        })
    }

    /// Decodes a `ProfileSwitch` frame into its structured payload.
    pub fn decode_profile_switch(&self) -> Option<ShmProfileSwitchPayload> {
        if self.kind() != ShmEventType::ProfileSwitch {
            return None;
        }
        Some(ShmProfileSwitchPayload {
            source_app: self.app(),
            app_id: unpack_game_launch_app_id(self.payload_b),
            platform_id: unpack_game_launch_platform_id(self.payload_b).unwrap_or(0),
            profile_hash: self.payload_a,
            profile_tag: self.tag_str().to_string(),
        })
    }

    /// Decodes a `CoOpInputP2` frame into its packed Player 2 controller state.
    pub fn decode_coop_input_p2(&self) -> Option<ShmCoOpInputP2Payload> {
        if self.kind() != ShmEventType::CoOpInputP2 {
            return None;
        }
        Some(ShmCoOpInputP2Payload {
            buttons: (self.payload_a & 0xFFFF) as u16,
            left_trigger: ((self.payload_a >> 16) & 0xFF) as u8,
            right_trigger: ((self.payload_a >> 24) & 0xFF) as u8,
            sequence: ((self.payload_a >> 32) & 0xFFFF_FFFF) as u32,
            thumb_lx: (self.payload_b & 0xFFFF) as u16 as i16,
            thumb_ly: ((self.payload_b >> 16) & 0xFFFF) as u16 as i16,
            thumb_rx: ((self.payload_b >> 32) & 0xFFFF) as u16 as i16,
            thumb_ry: ((self.payload_b >> 48) & 0xFFFF) as u16 as i16,
        })
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

/// Telemetry counters published by `duke-capture` into `Local\duke-capture-audio-tap-v1`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AudioTapStats {
    pub sys_rms: f32,
    pub mic_rms: f32,
    pub motion_score: f32,
    pub transient_epoch: u32,
    pub last_update_ms: u64,
    pub sys_write_idx: u64,
    pub mic_write_idx: u64,
}

#[repr(C, align(64))]
struct AudioTapRegion {
    magic: AtomicU64,
    sys_write_idx: AtomicU64,
    mic_write_idx: AtomicU64,
    sys_rms_bits: AtomicU32,
    mic_rms_bits: AtomicU32,
    motion_score_bits: AtomicU32,
    transient_epoch: AtomicU32,
    last_update_ms: AtomicU64,
    _pad: [u8; 16],
    sys_ring: [AtomicU16; AUDIO_TAP_RING_CAPACITY],
    mic_ring: [AtomicU16; AUDIO_TAP_RING_CAPACITY],
}

/// Lock-free 16 kHz shared-memory WASAPI loopback + microphone ring tap (~128 KiB).
pub struct DukeAudioTapShm {
    region: NonNull<AudioTapRegion>,
    #[cfg(target_os = "windows")]
    mapping_handle: windows::Win32::Foundation::HANDLE,
    #[cfg(not(target_os = "windows"))]
    _owned_backing: Option<Box<AudioTapRegion>>,
}

unsafe impl Send for DukeAudioTapShm {}
unsafe impl Sync for DukeAudioTapShm {}

impl DukeAudioTapShm {
    pub fn open_or_create() -> Result<Self, String> {
        Self::open_or_create_named(DUKE_AUDIO_TAP_SHM_NAME)
    }

    #[cfg(target_os = "windows")]
    pub fn open_or_create_named(mapping_name: &str) -> Result<Self, String> {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, INVALID_HANDLE_VALUE};
        use windows::Win32::System::Memory::{
            CreateFileMappingW, MapViewOfFile, FILE_MAP_ALL_ACCESS, PAGE_READWRITE,
        };

        let wide_name: Vec<u16> = OsStr::new(mapping_name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let region_bytes = std::mem::size_of::<AudioTapRegion>();
        let high = ((region_bytes as u64) >> 32) as u32;
        let low = (region_bytes as u64 & 0xFFFF_FFFF) as u32;

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

        let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        let mapped = unsafe { MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, region_bytes) };
        if mapped.Value.is_null() {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err(format!("MapViewOfFile('{}') returned null.", mapping_name));
        }

        let ptr = mapped.Value as *mut AudioTapRegion;
        let Some(region_nn) = NonNull::new(ptr) else {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err("Mapped AudioTap SHM pointer was null.".to_string());
        };

        if !already_exists {
            unsafe {
                std::ptr::write_bytes(ptr as *mut u8, 0, region_bytes);
                (*ptr).magic.store(AUDIO_TAP_MAGIC, Ordering::Release);
            }
        } else {
            for _ in 0..100 {
                if unsafe { (*ptr).magic.load(Ordering::Acquire) } == AUDIO_TAP_MAGIC {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_micros(50));
            }
        }

        Ok(Self {
            region: region_nn,
            mapping_handle: handle,
        })
    }

    #[cfg(not(target_os = "windows"))]
    pub fn open_or_create_named(_mapping_name: &str) -> Result<Self, String> {
        let region = Box::new(unsafe { MaybeUninit::<AudioTapRegion>::zeroed().assume_init() });
        region.magic.store(AUDIO_TAP_MAGIC, Ordering::Release);
        let ptr = NonNull::from(region.as_ref());
        Ok(Self {
            region: ptr,
            _owned_backing: Some(region),
        })
    }

    #[inline]
    fn region(&self) -> &AudioTapRegion {
        unsafe { self.region.as_ref() }
    }

    /// Downsamples interleaved stereo 48 kHz `f32` audio into the 16 kHz mono ring buffer.
    pub fn push_stereo_48k_decimated(&self, is_mic: bool, stereo_samples: &[f32]) -> f32 {
        if stereo_samples.len() < 6 {
            return 0.0;
        }
        let reg = self.region();
        let write_idx_atomic = if is_mic {
            &reg.mic_write_idx
        } else {
            &reg.sys_write_idx
        };
        let ring = if is_mic { &reg.mic_ring } else { &reg.sys_ring };
        let mut cursor = write_idx_atomic.load(Ordering::Acquire);
        let mask = AUDIO_TAP_RING_CAPACITY as u64 - 1;

        let mut sum_sq = 0.0f64;
        let mut count = 0usize;
        // 48 kHz stereo = 2 floats/frame; step 3 frames (6 floats) -> 16 kHz mono
        for idx in (0..stereo_samples.len().saturating_sub(5)).step_by(6) {
            let chunk = &stereo_samples[idx..idx + 6];
            let mono = (chunk[0] + chunk[1] + chunk[2] + chunk[3] + chunk[4] + chunk[5]) / 6.0;
            let clamped = mono.clamp(-1.0, 1.0);
            sum_sq += (clamped as f64) * (clamped as f64);
            count += 1;
            let s16 = (clamped * i16::MAX as f32).round() as i16;
            ring[(cursor & mask) as usize].store(s16 as u16, Ordering::Relaxed);
            cursor = cursor.wrapping_add(1);
        }
        fence(Ordering::Release);
        write_idx_atomic.store(cursor, Ordering::Release);

        let rms = if count > 0 {
            (sum_sq / count as f64).sqrt() as f32
        } else {
            0.0
        };
        if is_mic {
            reg.mic_rms_bits.store(rms.to_bits(), Ordering::Relaxed);
        } else {
            let prev_rms = f32::from_bits(reg.sys_rms_bits.load(Ordering::Relaxed));
            reg.sys_rms_bits.store(rms.to_bits(), Ordering::Relaxed);
            if rms > 0.28 && rms > prev_rms * 1.45 {
                reg.transient_epoch.fetch_add(1, Ordering::AcqRel);
            }
        }
        reg.last_update_ms
            .store(now_unix_micros() / 1000, Ordering::Relaxed);
        rms
    }

    pub fn set_motion_score(&self, score: f32) {
        let reg = self.region();
        reg.motion_score_bits
            .store(score.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        reg.last_update_ms
            .store(now_unix_micros() / 1000, Ordering::Relaxed);
    }

    pub fn read_stats(&self) -> AudioTapStats {
        let reg = self.region();
        AudioTapStats {
            sys_rms: f32::from_bits(reg.sys_rms_bits.load(Ordering::Relaxed)),
            mic_rms: f32::from_bits(reg.mic_rms_bits.load(Ordering::Relaxed)),
            motion_score: f32::from_bits(reg.motion_score_bits.load(Ordering::Relaxed)),
            transient_epoch: reg.transient_epoch.load(Ordering::Acquire),
            last_update_ms: reg.last_update_ms.load(Ordering::Acquire),
            sys_write_idx: reg.sys_write_idx.load(Ordering::Acquire),
            mic_write_idx: reg.mic_write_idx.load(Ordering::Acquire),
        }
    }

    /// Reads newly available 16 kHz mono float samples from both rings since caller's cursors.
    pub fn read_new_mono_samples(
        &self,
        sys_cursor: &mut u64,
        mic_cursor: &mut u64,
        max_samples: usize,
    ) -> (Vec<f32>, Vec<f32>) {
        let reg = self.region();
        let sys_out = Self::drain_ring(
            &reg.sys_write_idx,
            &reg.sys_ring,
            sys_cursor,
            max_samples,
        );
        let mic_out = Self::drain_ring(
            &reg.mic_write_idx,
            &reg.mic_ring,
            mic_cursor,
            max_samples,
        );
        (sys_out, mic_out)
    }

    fn drain_ring(
        write_idx: &AtomicU64,
        ring: &[AtomicU16; AUDIO_TAP_RING_CAPACITY],
        cursor: &mut u64,
        max_samples: usize,
    ) -> Vec<f32> {
        let head = write_idx.load(Ordering::Acquire);
        if head <= *cursor {
            if *cursor > head {
                *cursor = head;
            }
            return Vec::new();
        }
        let available = head - *cursor;
        let cap = AUDIO_TAP_RING_CAPACITY as u64;
        if available > cap {
            *cursor = head.saturating_sub(cap.min(max_samples as u64));
        }
        let count = (head - *cursor).min(max_samples as u64) as usize;
        let mask = cap - 1;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let slot = ((*cursor + i as u64) & mask) as usize;
            let raw = ring[slot].load(Ordering::Relaxed) as i16;
            out.push((raw as f32) / i16::MAX as f32);
        }
        *cursor += count as u64;
        out
    }
}

#[cfg(target_os = "windows")]
impl Drop for DukeAudioTapShm {
    fn drop(&mut self) {
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

    #[test]
    fn game_launch_payload_b_packs_sandbox_and_platform_id() {
        let pc_normal = pack_game_launch_payload_b(false, 0);
        assert!(!unpack_game_launch_sandbox(pc_normal));
        assert_eq!(unpack_game_launch_platform_id(pc_normal), None);

        let wii_sandbox = pack_game_launch_payload_b(true, 5);
        assert!(unpack_game_launch_sandbox(wii_sandbox));
        assert_eq!(unpack_game_launch_platform_id(wii_sandbox), Some(5));
        assert_eq!(console_label_for_platform_id(5), Some("Wii"));
        assert!(console_has_motion_gyro(5));
        assert!(!console_has_motion_gyro(1));
    }

    #[test]
    fn wave1_structured_shm_payloads_roundtrip() {
        let name = format!(r"Local\duke-suite-shm-wave1-{}", std::process::id());
        let producer = DukeShmBus::open_or_create_named(&name).unwrap();
        let mut consumer = DukeShmBus::open_or_create_named(&name).unwrap();

        let exe_hash = fnv1a_hash64(b"C:\\Games\\HadesII\\Hades2.exe");
        let launch_frame = ShmEventFrame::game_launched(
            ShmSourceApp::DukeLauncher,
            4242,
            1145350,
            7,
            true,
            exe_hash,
            "Hades II",
        );
        producer.publish_frame(launch_frame);

        let ach_frame = ShmEventFrame::achievement_unlocked(
            ShmSourceApp::DukeLauncher,
            4242,
            1145350,
            "ACH_NIGHT_CHAMPION",
        );
        producer.publish_frame(ach_frame);

        let prof_frame = ShmEventFrame::profile_switch(
            ShmSourceApp::DukeInput,
            0,
            1145350,
            7,
            "Switch-Gyro-Bezier5",
        );
        producer.publish_frame(prof_frame);

        let pad_state = ShmCoOpInputP2Payload {
            buttons: 0x1001,
            left_trigger: 128,
            right_trigger: 255,
            thumb_lx: -16384,
            thumb_ly: 32000,
            thumb_rx: 4096,
            thumb_ry: -8192,
            sequence: 99,
        };
        let coop_frame = ShmEventFrame::coop_input_p2(ShmSourceApp::DukeLauncher, 4242, pad_state);
        producer.publish_frame(coop_frame);

        let exit_frame = ShmEventFrame::game_exited(
            ShmSourceApp::DukeLauncher,
            4242,
            1145350,
            7,
            exe_hash,
            "Hades II",
        );
        producer.publish_frame(exit_frame);

        let peak_frame =
            ShmEventFrame::highlight_peak(ShmSourceApp::DukeLauncher, 4242, 92, 22, "BossPhase");
        producer.publish_frame(peak_frame);

        let voice_frame =
            ShmEventFrame::voice_macro(ShmSourceApp::DukeLauncher, 4242, 0x74, 40, "quick_save");
        producer.publish_frame(voice_frame);

        let rx1 = consumer.try_recv().unwrap().decode_game_launched().unwrap();
        assert_eq!(rx1.pid, 4242);
        assert_eq!(rx1.app_id, 1145350);
        assert_eq!(rx1.platform_id, 7);
        assert!(rx1.sandbox_enabled);
        assert_eq!(rx1.exe_hash, exe_hash);
        assert_eq!(rx1.tag, "Hades II");

        let rx2 = consumer
            .try_recv()
            .unwrap()
            .decode_achievement_unlocked()
            .unwrap();
        assert_eq!(rx2.app_id, 1145350);
        assert_eq!(rx2.title_slug, "ACH_NIGHT_CHAMPION");
        assert_eq!(
            rx2.title_slug_hash,
            fnv1a_hash64(b"ACH_NIGHT_CHAMPION")
        );

        let rx3 = consumer.try_recv().unwrap().decode_profile_switch().unwrap();
        assert_eq!(rx3.source_app, ShmSourceApp::DukeInput);
        assert_eq!(rx3.app_id, 1145350);
        assert_eq!(rx3.platform_id, 7);
        assert_eq!(rx3.profile_tag, "Switch-Gyro-Bezier5");

        let rx4 = consumer.try_recv().unwrap().decode_coop_input_p2().unwrap();
        assert_eq!(rx4, pad_state);

        let rx5 = consumer.try_recv().unwrap().decode_game_exited().unwrap();
        assert_eq!(rx5.app_id, 1145350);
        assert_eq!(rx5.exe_hash, exe_hash);

        let rx6 = consumer.try_recv().unwrap().decode_highlight_peak().unwrap();
        assert_eq!(rx6.0, 92);
        assert_eq!(rx6.1, 22);
        assert_eq!(rx6.2, "BossPhase");

        let rx7 = consumer.try_recv().unwrap().decode_voice_macro().unwrap();
        assert_eq!(rx7.0, 0x74);
        assert_eq!(rx7.1, 40);
        assert_eq!(rx7.2, "quick_save");
    }

    #[test]
    fn audio_tap_shm_decimates_and_streams_loopback_and_mic() {
        let name = format!(r"Local\duke-audio-tap-unit-{}", std::process::id());
        let writer = DukeAudioTapShm::open_or_create_named(&name).unwrap();
        let reader = DukeAudioTapShm::open_or_create_named(&name).unwrap();

        let mut stereo_sys = Vec::with_capacity(600);
        let mut stereo_mic = Vec::with_capacity(600);
        for i in 0..300 {
            let v_sys = ((i as f32 * 0.05).sin()) * 0.5;
            let v_mic = ((i as f32 * 0.08).cos()) * 0.25;
            stereo_sys.push(v_sys);
            stereo_sys.push(v_sys);
            stereo_mic.push(v_mic);
            stereo_mic.push(v_mic);
        }

        let rms_sys = writer.push_stereo_48k_decimated(false, &stereo_sys);
        let rms_mic = writer.push_stereo_48k_decimated(true, &stereo_mic);
        writer.set_motion_score(0.73);
        assert!(rms_sys > 0.1);
        assert!(rms_mic > 0.05);

        let stats = reader.read_stats();
        assert!((stats.motion_score - 0.73).abs() < 0.01);
        assert_eq!(stats.sys_write_idx, 100);
        assert_eq!(stats.mic_write_idx, 100);

        let mut c_sys = 0u64;
        let mut c_mic = 0u64;
        let (rx_sys, rx_mic) = reader.read_new_mono_samples(&mut c_sys, &mut c_mic, 256);
        assert_eq!(rx_sys.len(), 100);
        assert_eq!(rx_mic.len(), 100);
        assert_eq!(c_sys, 100);
        assert_eq!(c_mic, 100);
    }
}
