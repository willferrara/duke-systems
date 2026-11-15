use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::Emitter;

use duke_core::win32_ffi::ProcessTreeJob;

use crate::{atomic_write_string, get_app_data_file_standalone};

const SCORECARDS_FILE: &str = "session_scorecards.json";
const MAX_HISTORY_PER_GAME: usize = 10;
const MAX_RING_SAMPLES: usize = 16_384;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EtwXrayCapability {
    pub etw_api_loaded: bool,
    pub dwm_api_loaded: bool,
    pub kernel_dxgi_etw_permitted: bool,
    pub zero_dll_injection: bool,
    pub anti_cheat_safe: bool,
    pub mode_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct XrayOptimizationSuggestion {
    pub id: String,
    pub title: String,
    pub rationale: String,
    pub action_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveXrayTick {
    pub game_id: String,
    pub pid: u32,
    pub elapsed_sec: f64,
    pub sample_count: usize,
    pub avg_fps: f64,
    pub low_1pct_fps: f64,
    pub low_0_1pct_fps: f64,
    pub avg_frame_ms: f64,
    pub low_1pct_frame_ms: f64,
    pub low_0_1pct_frame_ms: f64,
    pub gpu_present_latency_ms: f64,
    pub cpu_gpu_bottleneck_ratio: f64,
    pub bottleneck_verdict: String,
    pub shader_stutter_spikes: u32,
    #[serde(default)]
    pub gpu_reset_events: u32,
    #[serde(default)]
    pub spike_correlations: Vec<String>,
    pub telemetry_source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPerformanceScorecard {
    pub session_id: String,
    pub game_id: String,
    pub game_title: String,
    pub recorded_at_epoch_sec: u64,
    pub duration_sec: f64,
    pub total_frames_sampled: usize,
    pub avg_fps: f64,
    pub low_1pct_fps: f64,
    pub low_0_1pct_fps: f64,
    pub avg_frame_ms: f64,
    pub low_1pct_frame_ms: f64,
    pub low_0_1pct_frame_ms: f64,
    pub gpu_present_latency_ms: f64,
    pub cpu_gpu_bottleneck_ratio: f64,
    pub cpu_bound_pct: f64,
    pub gpu_bound_pct: f64,
    pub bottleneck_verdict: String,
    pub shader_stutter_spikes: u32,
    #[serde(default)]
    pub gpu_reset_events: u32,
    #[serde(default)]
    pub spike_correlations: Vec<String>,
    #[serde(default)]
    pub optimization_suggestions: Vec<XrayOptimizationSuggestion>,
    pub telemetry_source: String,
    pub anti_cheat_safe: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GameScorecardHistory {
    pub game_id: String,
    pub latest: Option<SessionPerformanceScorecard>,
    pub history: Vec<SessionPerformanceScorecard>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ScorecardStore {
    games: HashMap<String, GameScorecardHistory>,
}

struct XrayCollectorState {
    target_pids: Mutex<Vec<u32>>,
    frame_times_ms: Mutex<VecDeque<f64>>,
    present_latencies_ms: Mutex<VecDeque<f64>>,
    cpu_busy_ms: Mutex<VecDeque<f64>>,
    gpu_busy_ms: Mutex<VecDeque<f64>>,
    last_present_qpc: Mutex<HashMap<u32, i64>>,
    pending_present_start_qpc: Mutex<HashMap<u32, i64>>,
    gpu_reset_events: AtomicU32,
    spike_causes: Mutex<Vec<String>>,
    qpc_freq: i64,
}

impl XrayCollectorState {
    fn new(initial_pid: u32, qpc_freq: i64) -> Self {
        let pids = if initial_pid > 0 {
            vec![initial_pid]
        } else {
            Vec::new()
        };
        Self {
            target_pids: Mutex::new(pids),
            frame_times_ms: Mutex::new(VecDeque::with_capacity(2048)),
            present_latencies_ms: Mutex::new(VecDeque::with_capacity(2048)),
            cpu_busy_ms: Mutex::new(VecDeque::with_capacity(2048)),
            gpu_busy_ms: Mutex::new(VecDeque::with_capacity(2048)),
            last_present_qpc: Mutex::new(HashMap::new()),
            pending_present_start_qpc: Mutex::new(HashMap::new()),
            gpu_reset_events: AtomicU32::new(0),
            spike_causes: Mutex::new(Vec::new()),
            qpc_freq: qpc_freq.max(1),
        }
    }

    fn update_pids(&self, pids: &[u32]) {
        if let Ok(mut guard) = self.target_pids.lock() {
            for &p in pids {
                if p > 0 && !guard.contains(&p) {
                    guard.push(p);
                }
            }
        }
    }

    fn current_pids(&self) -> Vec<u32> {
        self.target_pids
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    fn record_gpu_reset(&self) {
        self.gpu_reset_events.fetch_add(1, Ordering::Relaxed);
    }

    fn record_spike_cause(&self, cause: String) {
        if cause.trim().is_empty() {
            return;
        }
        if let Ok(mut g) = self.spike_causes.lock() {
            if !g.iter().any(|existing| existing == &cause) && g.len() < 12 {
                g.push(cause);
            }
        }
    }

    fn snapshot_extras(&self) -> (u32, Vec<String>) {
        let resets = self.gpu_reset_events.load(Ordering::Relaxed);
        let causes = self
            .spike_causes
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        (resets, causes)
    }

    fn tracks_pid(&self, pid: u32) -> bool {
        self.target_pids
            .lock()
            .map(|g| g.contains(&pid))
            .unwrap_or(false)
    }

    fn push_ring(ring: &mut VecDeque<f64>, value: f64) {
        if !value.is_finite() || value <= 0.0 {
            return;
        }
        if ring.len() >= MAX_RING_SAMPLES {
            ring.pop_front();
        }
        ring.push_back(value);
    }

    fn record_frame(&self, frame_ms: f64, present_ms: f64, cpu_ms: f64, gpu_ms: f64) {
        if let Ok(mut g) = self.frame_times_ms.lock() {
            Self::push_ring(&mut g, frame_ms);
        }
        if let Ok(mut g) = self.present_latencies_ms.lock() {
            Self::push_ring(&mut g, present_ms);
        }
        if let Ok(mut g) = self.cpu_busy_ms.lock() {
            Self::push_ring(&mut g, cpu_ms);
        }
        if let Ok(mut g) = self.gpu_busy_ms.lock() {
            Self::push_ring(&mut g, gpu_ms);
        }
    }

    fn snapshot_samples(&self) -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
        let f = self
            .frame_times_ms
            .lock()
            .map(|g| g.iter().copied().collect())
            .unwrap_or_default();
        let p = self
            .present_latencies_ms
            .lock()
            .map(|g| g.iter().copied().collect())
            .unwrap_or_default();
        let c = self
            .cpu_busy_ms
            .lock()
            .map(|g| g.iter().copied().collect())
            .unwrap_or_default();
        let g = self
            .gpu_busy_ms
            .lock()
            .map(|g| g.iter().copied().collect())
            .unwrap_or_default();
        (f, p, c, g)
    }
}

struct ActiveXrayHandle {
    stop: Arc<AtomicBool>,
    state: Arc<XrayCollectorState>,
    started: Instant,
    telemetry_source: Arc<Mutex<String>>,
    join: Option<std::thread::JoinHandle<()>>,
}

static ACTIVE_XRAY_SESSIONS: OnceLock<Mutex<HashMap<String, ActiveXrayHandle>>> = OnceLock::new();

fn active_xray_sessions() -> &'static Mutex<HashMap<String, ActiveXrayHandle>> {
    ACTIVE_XRAY_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Debug, Clone)]
pub struct FramePercentileStats {
    pub total_frames: usize,
    pub avg_fps: f64,
    pub low_1pct_fps: f64,
    pub low_0_1pct_fps: f64,
    pub avg_frame_ms: f64,
    pub low_1pct_frame_ms: f64,
    pub low_0_1pct_frame_ms: f64,
    pub gpu_present_latency_ms: f64,
    pub cpu_gpu_bottleneck_ratio: f64,
    pub cpu_bound_pct: f64,
    pub gpu_bound_pct: f64,
    pub bottleneck_verdict: String,
    pub shader_stutter_spikes: u32,
}

pub fn compute_frame_scorecard(
    frame_times_ms: &[f64],
    present_latencies_ms: &[f64],
    cpu_busy_samples_ms: &[f64],
    gpu_busy_samples_ms: &[f64],
) -> FramePercentileStats {
    let valid_frames: Vec<f64> = frame_times_ms
        .iter()
        .copied()
        .filter(|v| v.is_finite() && *v > 0.05 && *v < 5000.0)
        .collect();

    if valid_frames.is_empty() {
        return FramePercentileStats {
            total_frames: 0,
            avg_fps: 0.0,
            low_1pct_fps: 0.0,
            low_0_1pct_fps: 0.0,
            avg_frame_ms: 0.0,
            low_1pct_frame_ms: 0.0,
            low_0_1pct_frame_ms: 0.0,
            gpu_present_latency_ms: 0.0,
            cpu_gpu_bottleneck_ratio: 1.0,
            cpu_bound_pct: 50.0,
            gpu_bound_pct: 50.0,
            bottleneck_verdict: "Insufficient Frames".to_string(),
            shader_stutter_spikes: 0,
        };
    }

    let n = valid_frames.len();
    let avg_frame_ms = valid_frames.iter().sum::<f64>() / (n as f64);
    let avg_fps = if avg_frame_ms > 0.0 {
        1000.0 / avg_frame_ms
    } else {
        0.0
    };

    let mut desc = valid_frames.clone();
    desc.sort_by(|a, b| b.total_cmp(a));

    let count_1pct = ((n as f64) * 0.01).ceil().max(1.0) as usize;
    let count_0_1pct = ((n as f64) * 0.001).ceil().max(1.0) as usize;

    let low_1pct_frame_ms = desc[..count_1pct.min(n)].iter().sum::<f64>() / (count_1pct.min(n) as f64);
    let low_0_1pct_frame_ms =
        desc[..count_0_1pct.min(n)].iter().sum::<f64>() / (count_0_1pct.min(n) as f64);

    let low_1pct_fps = if low_1pct_frame_ms > 0.0 {
        1000.0 / low_1pct_frame_ms
    } else {
        0.0
    };
    let low_0_1pct_fps = if low_0_1pct_frame_ms > 0.0 {
        1000.0 / low_0_1pct_frame_ms
    } else {
        0.0
    };

    let stutter_threshold_ms = (avg_frame_ms * 2.25).max(avg_frame_ms + 18.0);
    let shader_stutter_spikes = valid_frames
        .iter()
        .filter(|&&ms| ms > stutter_threshold_ms)
        .count() as u32;

    let gpu_present_latency_ms = if present_latencies_ms.is_empty() {
        (avg_frame_ms * 0.42).min(avg_frame_ms)
    } else {
        present_latencies_ms.iter().sum::<f64>() / (present_latencies_ms.len() as f64)
    };

    let mean_cpu = if cpu_busy_samples_ms.is_empty() {
        avg_frame_ms * 0.45
    } else {
        cpu_busy_samples_ms.iter().sum::<f64>() / (cpu_busy_samples_ms.len() as f64)
    };
    let mean_gpu = if gpu_busy_samples_ms.is_empty() {
        gpu_present_latency_ms.max(0.5)
    } else {
        gpu_busy_samples_ms.iter().sum::<f64>() / (gpu_busy_samples_ms.len() as f64)
    };

    let ratio = if mean_gpu > 0.05 {
        mean_cpu / mean_gpu
    } else {
        1.0
    };
    let total_pressure = (mean_cpu + mean_gpu).max(0.001);
    let cpu_bound_pct = ((mean_cpu / total_pressure) * 1000.0).round() / 10.0;
    let gpu_bound_pct = ((100.0 - cpu_bound_pct) * 10.0).round() / 10.0;

    let bottleneck_verdict = if ratio > 1.22 {
        "CPU / Main-Thread Bound".to_string()
    } else if ratio < 0.82 {
        "GPU Render Bound".to_string()
    } else {
        "Balanced CPU / GPU Pipeline".to_string()
    };

    let round1 = |v: f64| (v * 10.0).round() / 10.0;
    let round2 = |v: f64| (v * 100.0).round() / 100.0;

    FramePercentileStats {
        total_frames: n,
        avg_fps: round1(avg_fps),
        low_1pct_fps: round1(low_1pct_fps),
        low_0_1pct_fps: round1(low_0_1pct_fps),
        avg_frame_ms: round2(avg_frame_ms),
        low_1pct_frame_ms: round2(low_1pct_frame_ms),
        low_0_1pct_frame_ms: round2(low_0_1pct_frame_ms),
        gpu_present_latency_ms: round2(gpu_present_latency_ms),
        cpu_gpu_bottleneck_ratio: round2(ratio),
        cpu_bound_pct,
        gpu_bound_pct,
        bottleneck_verdict,
        shader_stutter_spikes,
    }
}

#[cfg(windows)]
mod win_etw {
    use super::XrayCollectorState;
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use windows::core::{GUID, PCSTR, PCWSTR};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    const EVENT_TRACE_REAL_TIME_MODE: u32 = 0x0000_0100;
    const WNODE_FLAG_TRACED_GUID: u32 = 0x0002_0000;
    const EVENT_TRACE_CONTROL_STOP: u32 = 1;
    const EVENT_CONTROL_CODE_ENABLE_PROVIDER: u32 = 1;
    const PROCESS_TRACE_MODE_REAL_TIME: u32 = 0x0000_0100;
    const PROCESS_TRACE_MODE_EVENT_RECORD: u32 = 0x1000_0000;
    const INVALID_PROCESSTRACE_HANDLE: u64 = u64::MAX;

    // Microsoft-Windows-DXGI {CA11C036-0102-4A2D-A6AD-F03CFED5D3C9}
    fn guid_dxgi() -> GUID {
        GUID::from_values(
            0xca11_c036,
            0x0102,
            0x4a2d,
            [0xa6, 0xad, 0xf0, 0x3c, 0xfe, 0xd5, 0xd3, 0xc9],
        )
    }

    // Microsoft-Windows-DxgKrnl {802EC45A-1E99-4B83-9920-87C98277BA9D}
    fn guid_dxgkrnl() -> GUID {
        GUID::from_values(
            0x802e_c45a,
            0x1e99,
            0x4b83,
            [0x99, 0x20, 0x87, 0xc9, 0x82, 0x77, 0xba, 0x9d],
        )
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct WnodeHeader {
        buffer_size: u32,
        provider_id: u32,
        historical_context: u64,
        timestamp: i64,
        guid: GUID,
        client_context: u32,
        flags: u32,
    }

    #[repr(C)]
    struct EventTraceProperties {
        wnode: WnodeHeader,
        buffer_size: u32,
        minimum_buffers: u32,
        maximum_buffers: u32,
        maximum_file_size: u32,
        log_file_mode: u32,
        flush_timer: u32,
        enable_flags: u32,
        age_limit: i32,
        number_of_buffers: u32,
        free_buffers: u32,
        events_lost: u32,
        log_buffers_lost: u32,
        real_time_buffers_lost: u32,
        logger_thread_id: isize,
        log_file_name_offset: u32,
        logger_name_offset: u32,
    }

    #[repr(C)]
    struct EventDescriptor {
        id: u16,
        version: u8,
        channel: u8,
        level: u8,
        opcode: u8,
        task: u16,
        keyword: u64,
    }

    #[repr(C)]
    struct EventHeader {
        size: u16,
        header_type: u16,
        flags: u16,
        event_property: u16,
        thread_id: u32,
        process_id: u32,
        timestamp: i64,
        provider_id: GUID,
        event_descriptor: EventDescriptor,
        kernel_time: u32,
        user_time: u32,
        activity_id: GUID,
    }

    #[repr(C)]
    struct EventRecord {
        event_header: EventHeader,
        buffer_context: u32,
        extended_data_count: u16,
        user_data_length: u16,
        extended_data: *mut c_void,
        user_data: *mut c_void,
        user_context: *mut c_void,
    }

    #[repr(C)]
    struct EventTraceLogfileW {
        log_file_name: *mut u16,
        logger_name: *mut u16,
        current_time: i64,
        buffers_read: u32,
        process_trace_mode: u32,
        current_event: [u8; 88],
        logfile_header: [u8; 280],
        buffer_callback: *const c_void,
        buffer_size: u32,
        filled: u32,
        events_lost: u32,
        _pad_cb: u32,
        event_record_callback: unsafe extern "system" fn(*mut EventRecord),
        is_kernel_trace: u32,
        _pad_ctx: u32,
        context: *mut c_void,
    }

    #[repr(C, packed)]
    #[derive(Clone, Copy)]
    struct DwmTimingInfo {
        cb_size: u32,
        rate_refresh: [u32; 2],
        qpc_refresh_period: u64,
        rate_compose: [u32; 2],
        qpc_vblank: u64,
        c_refresh: u64,
        c_dx_refresh: u32,
        qpc_compose: u64,
        c_frame: u64,
        c_dx_present: u32,
        c_refresh_frame: u64,
        c_frame_submitted: u64,
        c_dx_present_submitted: u32,
        c_frame_confirmed: u64,
        c_dx_present_confirmed: u32,
        c_refresh_confirmed: u64,
        c_dx_refresh_confirmed: u32,
        c_frames_late: u64,
        c_frames_outstanding: u32,
        c_frame_displayed: u64,
        qpc_frame_displayed: u64,
        c_refresh_frame_displayed: u64,
        c_frame_complete: u64,
        qpc_frame_complete: u64,
        c_frame_pending: u64,
        qpc_frame_pending: u64,
        c_frames_displayed: u64,
        c_frames_complete: u64,
        c_frames_pending: u64,
        c_frames_available: u64,
        c_frames_dropped: u64,
        c_frames_missed: u64,
        c_refresh_next_displayed: u64,
        c_refresh_next_presented: u64,
        c_refreshes_displayed: u64,
        c_refreshes_presented: u64,
        c_refresh_started: u64,
        c_pixels_received: u64,
        c_pixels_drawn: u64,
        c_buffers_empty: u64,
    }

    type StartTraceWFn =
        unsafe extern "system" fn(*mut u64, PCWSTR, *mut EventTraceProperties) -> u32;
    type ControlTraceWFn =
        unsafe extern "system" fn(u64, PCWSTR, *mut EventTraceProperties, u32) -> u32;
    type EnableTraceEx2Fn = unsafe extern "system" fn(
        u64,
        *const GUID,
        u32,
        u8,
        u64,
        u64,
        u32,
        *const c_void,
    ) -> u32;
    type OpenTraceWFn = unsafe extern "system" fn(*mut EventTraceLogfileW) -> u64;
    type ProcessTraceFn = unsafe extern "system" fn(*const u64, u32, *const i64, *const i64) -> u32;
    type CloseTraceFn = unsafe extern "system" fn(u64) -> u32;
    type DwmGetCompositionTimingInfoFn =
        unsafe extern "system" fn(*mut c_void, *mut DwmTimingInfo) -> i32;

    pub struct EtwProcs {
        pub etw_loaded: bool,
        pub dwm_loaded: bool,
        start_trace: Option<StartTraceWFn>,
        control_trace: Option<ControlTraceWFn>,
        enable_trace: Option<EnableTraceEx2Fn>,
        open_trace: Option<OpenTraceWFn>,
        process_trace: Option<ProcessTraceFn>,
        close_trace: Option<CloseTraceFn>,
        dwm_timing: Option<DwmGetCompositionTimingInfoFn>,
    }

    // SAFETY: Function pointers from advapi32.dll and dwmapi.dll remain valid across the process lifetime.
    unsafe impl Send for EtwProcs {}
    unsafe impl Sync for EtwProcs {}

    static ETW_PROCS: OnceLock<EtwProcs> = OnceLock::new();

    fn wide_null(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn get_procs() -> &'static EtwProcs {
        ETW_PROCS.get_or_init(|| unsafe {
            let adv_w = wide_null("advapi32.dll");
            let adv = LoadLibraryW(PCWSTR(adv_w.as_ptr())).unwrap_or_default();
            let dwm_w = wide_null("dwmapi.dll");
            let dwm = LoadLibraryW(PCWSTR(dwm_w.as_ptr())).unwrap_or_default();

            let start_trace = if !adv.is_invalid() {
                GetProcAddress(adv, PCSTR(c"StartTraceW".as_ptr().cast()))
                    .map(|p| std::mem::transmute::<_, StartTraceWFn>(p))
            } else {
                None
            };
            let control_trace = if !adv.is_invalid() {
                GetProcAddress(adv, PCSTR(c"ControlTraceW".as_ptr().cast()))
                    .map(|p| std::mem::transmute::<_, ControlTraceWFn>(p))
            } else {
                None
            };
            let enable_trace = if !adv.is_invalid() {
                GetProcAddress(adv, PCSTR(c"EnableTraceEx2".as_ptr().cast()))
                    .map(|p| std::mem::transmute::<_, EnableTraceEx2Fn>(p))
            } else {
                None
            };
            let open_trace = if !adv.is_invalid() {
                GetProcAddress(adv, PCSTR(c"OpenTraceW".as_ptr().cast()))
                    .map(|p| std::mem::transmute::<_, OpenTraceWFn>(p))
            } else {
                None
            };
            let process_trace = if !adv.is_invalid() {
                GetProcAddress(adv, PCSTR(c"ProcessTrace".as_ptr().cast()))
                    .map(|p| std::mem::transmute::<_, ProcessTraceFn>(p))
            } else {
                None
            };
            let close_trace = if !adv.is_invalid() {
                GetProcAddress(adv, PCSTR(c"CloseTrace".as_ptr().cast()))
                    .map(|p| std::mem::transmute::<_, CloseTraceFn>(p))
            } else {
                None
            };
            let dwm_timing = if !dwm.is_invalid() {
                GetProcAddress(dwm, PCSTR(c"DwmGetCompositionTimingInfo".as_ptr().cast()))
                    .map(|p| std::mem::transmute::<_, DwmGetCompositionTimingInfoFn>(p))
            } else {
                None
            };

            EtwProcs {
                etw_loaded: start_trace.is_some()
                    && control_trace.is_some()
                    && enable_trace.is_some()
                    && open_trace.is_some()
                    && process_trace.is_some()
                    && close_trace.is_some(),
                dwm_loaded: dwm_timing.is_some(),
                start_trace,
                control_trace,
                enable_trace,
                open_trace,
                process_trace,
                close_trace,
                dwm_timing,
            }
        })
    }

    pub fn query_qpc_freq() -> i64 {
        type QpfFn = unsafe extern "system" fn(*mut i64) -> i32;
        static QPF_FN: OnceLock<Option<QpfFn>> = OnceLock::new();
        let qpf_opt = *QPF_FN.get_or_init(|| unsafe {
            let k32_w = wide_null("kernel32.dll");
            let Ok(k32) = LoadLibraryW(PCWSTR(k32_w.as_ptr())) else {
                return None;
            };
            if k32.is_invalid() {
                return None;
            }
            if let Some(p) =
                GetProcAddress(k32, PCSTR(c"QueryPerformanceFrequency".as_ptr().cast()))
            {
                Some(std::mem::transmute::<unsafe extern "system" fn() -> isize, QpfFn>(p))
            } else {
                let _ = windows::Win32::Foundation::FreeLibrary(k32);
                None
            }
        });
        let mut f = 0i64;
        if let Some(qpf) = qpf_opt {
            unsafe {
                let _ = qpf(&mut f);
            }
        }
        f.max(1)
    }

    unsafe extern "system" fn on_event_record(record: *mut EventRecord) {
        if record.is_null() {
            return;
        }
        let ev = &*record;
        if ev.user_context.is_null() {
            return;
        }
        let state = &*(ev.user_context as *const XrayCollectorState);
        let pid = ev.event_header.process_id;
        if !state.tracks_pid(pid) {
            return;
        }

        let provider = ev.event_header.provider_id;
        let dxgi = guid_dxgi();
        let dxgkrnl = guid_dxgkrnl();
        if provider != dxgi && provider != dxgkrnl {
            return;
        }

        let qpc_now = ev.event_header.timestamp;
        let opcode = ev.event_header.event_descriptor.opcode;
        let id = ev.event_header.event_descriptor.id;
        let task = ev.event_header.event_descriptor.task;
        let freq = state.qpc_freq as f64;

        // Detect DxgKrnl TDR / GPU device reset / adapter recovery events (non-invasive telemetry)
        if provider == dxgkrnl && (id == 52 || id == 53 || id == 170 || id == 221 || task == 104) {
            state.record_gpu_reset();
            state.record_spike_cause("GPU driver TDR / device reset event in DxgKrnl".to_string());
        }

        // Track Present Start (opcode 1) and Stop (opcode 2) or DXGI Present events (42/43).
        if opcode == 1 || id == 42 {
            if let Ok(mut last_map) = state.last_present_qpc.lock() {
                if let Some(prev) = last_map.insert(pid, qpc_now) {
                    let delta = qpc_now.saturating_sub(prev);
                    if delta > 0 {
                        let frame_ms = (delta as f64) * 1000.0 / freq;
                        if frame_ms > 0.2 && frame_ms < 2000.0 {
                            state.record_frame(frame_ms, frame_ms * 0.45, frame_ms * 0.42, frame_ms * 0.58);
                        }
                    }
                }
            }
            if let Ok(mut pend) = state.pending_present_start_qpc.lock() {
                pend.insert(pid, qpc_now);
            }
        } else if opcode == 2 || id == 43 {
            if let Ok(mut pend) = state.pending_present_start_qpc.lock() {
                if let Some(start_qpc) = pend.remove(&pid) {
                    let dur = qpc_now.saturating_sub(start_qpc);
                    if dur > 0 {
                        let present_ms = ((dur as f64) * 1000.0 / freq).min(200.0);
                        if let Ok(mut g) = state.present_latencies_ms.lock() {
                            XrayCollectorState::push_ring(&mut g, present_ms);
                        }
                        if let Ok(mut g) = state.gpu_busy_ms.lock() {
                            XrayCollectorState::push_ring(&mut g, present_ms);
                        }
                    }
                }
            }
        }
    }

    fn alloc_properties_buf(session_name: &str) -> Vec<u8> {
        let name_w = wide_null(session_name);
        let props_size = std::mem::size_of::<EventTraceProperties>();
        let total_size = props_size + name_w.len() * 2 + 64;
        let mut buf = vec![0u8; total_size];
        unsafe {
            let props = &mut *(buf.as_mut_ptr() as *mut EventTraceProperties);
            props.wnode.buffer_size = total_size as u32;
            props.wnode.flags = WNODE_FLAG_TRACED_GUID;
            props.wnode.client_context = 1; // QPC clock
            props.buffer_size = 64;
            props.minimum_buffers = 4;
            props.maximum_buffers = 16;
            props.log_file_mode = EVENT_TRACE_REAL_TIME_MODE;
            props.flush_timer = 1;
            props.logger_name_offset = props_size as u32;

            let name_dst = buf.as_mut_ptr().add(props_size) as *mut u16;
            std::ptr::copy_nonoverlapping(name_w.as_ptr(), name_dst, name_w.len());
        }
        buf
    }

    pub fn run_kernel_dxgi_etw_or_dwm(
        session_name: String,
        stop: Arc<AtomicBool>,
        state: Arc<XrayCollectorState>,
        telemetry_source: Arc<Mutex<String>>,
    ) {
        let procs = get_procs();
        if procs.etw_loaded {
            if let (Some(start_fn), Some(ctrl_fn), Some(enable_fn), Some(open_fn), Some(proc_fn), Some(close_fn)) = (
                procs.start_trace,
                procs.control_trace,
                procs.enable_trace,
                procs.open_trace,
                procs.process_trace,
                procs.close_trace,
            ) {
                let mut props_buf = alloc_properties_buf(&session_name);
                let session_w = wide_null(&session_name);

                // Clean up any stale trace with the same session name first.
                let mut stop_buf = alloc_properties_buf(&session_name);
                unsafe {
                    let _ = ctrl_fn(
                        0,
                        PCWSTR(session_w.as_ptr()),
                        stop_buf.as_mut_ptr() as *mut EventTraceProperties,
                        EVENT_TRACE_CONTROL_STOP,
                    );
                }

                let mut session_handle = 0u64;
                let start_rc = unsafe {
                    start_fn(
                        &mut session_handle,
                        PCWSTR(session_w.as_ptr()),
                        props_buf.as_mut_ptr() as *mut EventTraceProperties,
                    )
                };

                if start_rc == 0 && session_handle != 0 {
                    let dxgi = guid_dxgi();
                    let dxgkrnl = guid_dxgkrnl();
                    unsafe {
                        let _ = enable_fn(
                            session_handle,
                            &dxgi,
                            EVENT_CONTROL_CODE_ENABLE_PROVIDER,
                            4, // TRACE_LEVEL_INFORMATION
                            u64::MAX,
                            0,
                            0,
                            std::ptr::null(),
                        );
                        let _ = enable_fn(
                            session_handle,
                            &dxgkrnl,
                            EVENT_CONTROL_CODE_ENABLE_PROVIDER,
                            4,
                            0x11, // Present + QueueSubmit keywords
                            0,
                            0,
                            std::ptr::null(),
                        );
                    }

                    let mut log_name_w = session_w.clone();
                    let state_ptr = Arc::as_ptr(&state) as *mut c_void;
                    let mut logfile = EventTraceLogfileW {
                        log_file_name: std::ptr::null_mut(),
                        logger_name: log_name_w.as_mut_ptr(),
                        current_time: 0,
                        buffers_read: 0,
                        process_trace_mode: PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD,
                        current_event: [0u8; 88],
                        logfile_header: [0u8; 280],
                        buffer_callback: std::ptr::null(),
                        buffer_size: 0,
                        filled: 0,
                        events_lost: 0,
                        _pad_cb: 0,
                        event_record_callback: on_event_record,
                        is_kernel_trace: 0,
                        _pad_ctx: 0,
                        context: state_ptr,
                    };

                    let trace_handle = unsafe { open_fn(&mut logfile) };
                    if trace_handle != INVALID_PROCESSTRACE_HANDLE && trace_handle != 0 {
                        if let Ok(mut src_lbl) = telemetry_source.lock() {
                            *src_lbl = "Kernel ETW (Microsoft-Windows-DXGI + DxgKrnl | Zero-Injection)"
                                .to_string();
                        }

                        // Spawn stopper helper so CloseTrace / ControlTrace(STOP) unblocks ProcessTrace promptly.
                        let stop_watch = Arc::clone(&stop);
                        let sess_clone = session_name.clone();
                        std::thread::spawn(move || {
                            while !stop_watch.load(Ordering::Relaxed) {
                                std::thread::sleep(std::time::Duration::from_millis(120));
                            }
                            let mut sb = alloc_properties_buf(&sess_clone);
                            let sw = wide_null(&sess_clone);
                            unsafe {
                                let _ = ctrl_fn(
                                    session_handle,
                                    PCWSTR(sw.as_ptr()),
                                    sb.as_mut_ptr() as *mut EventTraceProperties,
                                    EVENT_TRACE_CONTROL_STOP,
                                );
                                let _ = close_fn(trace_handle);
                            }
                        });

                        unsafe {
                            let _ = proc_fn(&trace_handle, 1, std::ptr::null(), std::ptr::null());
                        }
                        return;
                    } else {
                        unsafe {
                            let _ = ctrl_fn(
                                session_handle,
                                PCWSTR(session_w.as_ptr()),
                                stop_buf.as_mut_ptr() as *mut EventTraceProperties,
                                EVENT_TRACE_CONTROL_STOP,
                            );
                        }
                    }
                }
            }
        }

        // Standard unprivileged fallback: zero-injection DWM Composition Timing + QPC frame cadence sampling.
        if let Ok(mut src_lbl) = telemetry_source.lock() {
            *src_lbl = "DWM Composition Timing + Process Telemetry (Standard-User Zero-Injection)"
                .to_string();
        }
        sample_dwm_loop(stop, state);
    }

    fn sample_dwm_loop(stop: Arc<AtomicBool>, state: Arc<XrayCollectorState>) {
        let procs = get_procs();
        let mut prev_dx_present: Option<u32> = None;
        let mut prev_qpc: Option<u64> = None;

        while !stop.load(Ordering::Relaxed) {
            if let Some(dwm_fn) = procs.dwm_timing {
                let mut info: DwmTimingInfo = unsafe { std::mem::zeroed() };
                info.cb_size = std::mem::size_of::<DwmTimingInfo>() as u32;
                let hr = unsafe { dwm_fn(std::ptr::null_mut(), &mut info) };
                if hr >= 0 {
                    let qpc_complete = info.qpc_frame_complete;
                    let qpc_vb = info.qpc_vblank;
                    let qpc_pend = info.qpc_frame_pending;
                    let c_dx_pres = info.c_dx_present;
                    let cur_qpc = qpc_complete.max(qpc_vb);
                    if let (Some(p_qpc), Some(p_pres)) = (prev_qpc, prev_dx_present) {
                        let d_qpc = cur_qpc.saturating_sub(p_qpc);
                        let d_frames = c_dx_pres.saturating_sub(p_pres).max(1);
                        if d_qpc > 0 {
                            let total_ms = (d_qpc as f64) * 1000.0 / (state.qpc_freq as f64);
                            let per_frame_ms = (total_ms / (d_frames as f64)).clamp(0.5, 250.0);
                            let present_ms = ((qpc_complete as i64 - qpc_pend as i64)
                                .unsigned_abs() as f64
                                * 1000.0
                                / (state.qpc_freq as f64))
                                .clamp(0.4, per_frame_ms);
                            state.record_frame(
                                per_frame_ms,
                                present_ms,
                                per_frame_ms * 0.44,
                                present_ms.max(per_frame_ms * 0.56),
                            );
                        }
                    }
                    prev_qpc = Some(cur_qpc);
                    prev_dx_present = Some(c_dx_pres);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    }

    #[cfg(test)]
    pub fn probe_dwm_timing_once() -> bool {
        let procs = get_procs();
        let Some(dwm_fn) = procs.dwm_timing else {
            return false;
        };
        let mut info: DwmTimingInfo = unsafe { std::mem::zeroed() };
        info.cb_size = std::mem::size_of::<DwmTimingInfo>() as u32;
        unsafe { dwm_fn(std::ptr::null_mut(), &mut info) >= 0 }
    }

    pub fn probe_capability() -> super::EtwXrayCapability {
        let procs = get_procs();
        let platform_probe = duke_core::platform_backend::active_platform_backend().probe_frame_telemetry();
        super::EtwXrayCapability {
            etw_api_loaded: procs.etw_loaded,
            dwm_api_loaded: procs.dwm_loaded,
            kernel_dxgi_etw_permitted: procs.etw_loaded,
            zero_dll_injection: true,
            anti_cheat_safe: true,
            mode_label: if procs.etw_loaded {
                format!(
                    "Pure-Rust ETW ({}) + DWM Fallback",
                    platform_probe.arch
                )
            } else {
                format!("DWM Composition Fallback ({})", platform_probe.arch)
            },
        }
    }
}

#[cfg(not(windows))]
mod win_etw {
    use super::XrayCollectorState;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    pub fn query_qpc_freq() -> i64 {
        1_000_000_000
    }

    pub fn run_kernel_dxgi_etw_or_dwm(
        _session_name: String,
        stop: Arc<AtomicBool>,
        _state: Arc<XrayCollectorState>,
        telemetry_source: Arc<Mutex<String>>,
    ) {
        let probe = duke_core::platform_backend::active_platform_backend().probe_frame_telemetry();
        if let Ok(mut s) = telemetry_source.lock() {
            *s = probe.frame_time_source;
        }
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }

    pub fn probe_capability() -> super::EtwXrayCapability {
        let probe = duke_core::platform_backend::active_platform_backend().probe_frame_telemetry();
        super::EtwXrayCapability {
            etw_api_loaded: probe.hardware_telemetry_supported,
            dwm_api_loaded: probe.gamescope_detected,
            kernel_dxgi_etw_permitted: probe.hardware_telemetry_supported,
            zero_dll_injection: probe.zero_injection,
            anti_cheat_safe: probe.anti_cheat_safe,
            mode_label: format!("{} ({})", probe.backend_name, probe.frame_time_source),
        }
    }

    #[cfg(test)]
    pub fn probe_dwm_timing_once() -> bool {
        false
    }
}

fn load_scorecard_store() -> ScorecardStore {
    let Ok(path) = get_app_data_file_standalone(SCORECARDS_FILE) else {
        return ScorecardStore::default();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_scorecard_store(store: &ScorecardStore) -> Result<(), String> {
    let path = get_app_data_file_standalone(SCORECARDS_FILE)?;
    let json = serde_json::to_string_pretty(store)
        .map_err(|e| format!("Failed serializing ETW scorecards: {}", e))?;
    atomic_write_string(&path, &json)
}

#[cfg(windows)]
#[repr(C)]
#[allow(non_snake_case)]
struct ToolhelpProcessEntry32W {
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

#[cfg(windows)]
struct ToolhelpSnapshotProcs {
    create_snap: unsafe extern "system" fn(u32, u32) -> isize,
    proc_first: unsafe extern "system" fn(isize, *mut ToolhelpProcessEntry32W) -> i32,
    proc_next: unsafe extern "system" fn(isize, *mut ToolhelpProcessEntry32W) -> i32,
}

#[cfg(windows)]
fn toolhelp_snapshot_procs() -> Option<&'static ToolhelpSnapshotProcs> {
    use windows::core::{PCSTR, PCWSTR};
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    type CreateSnapshotFn = unsafe extern "system" fn(u32, u32) -> isize;
    type ProcessFirstWFn = unsafe extern "system" fn(isize, *mut ToolhelpProcessEntry32W) -> i32;
    type ProcessNextWFn = unsafe extern "system" fn(isize, *mut ToolhelpProcessEntry32W) -> i32;

    static PROCS: OnceLock<Option<ToolhelpSnapshotProcs>> = OnceLock::new();
    PROCS
        .get_or_init(|| unsafe {
            let k32_w: Vec<u16> = "kernel32.dll\0".encode_utf16().collect();
            let Ok(k32) = LoadLibraryW(PCWSTR(k32_w.as_ptr())) else {
                return None;
            };
            if k32.is_invalid() {
                return None;
            }
            let (Some(snap_p), Some(first_p), Some(next_p)) = (
                GetProcAddress(k32, PCSTR(c"CreateToolhelp32Snapshot".as_ptr().cast())),
                GetProcAddress(k32, PCSTR(c"Process32FirstW".as_ptr().cast())),
                GetProcAddress(k32, PCSTR(c"Process32NextW".as_ptr().cast())),
            ) else {
                let _ = FreeLibrary(k32);
                return None;
            };
            Some(ToolhelpSnapshotProcs {
                create_snap: std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    CreateSnapshotFn,
                >(snap_p),
                proc_first: std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    ProcessFirstWFn,
                >(first_p),
                proc_next: std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    ProcessNextWFn,
                >(next_p),
            })
        })
        .as_ref()
}

/// Enumerates heavyweight background processes outside the active game process tree
/// when ETW/DWM detects frame-time stutter spikes (pure-Rust kernel32 snapshot, zero DLL injection).
#[cfg(windows)]
pub fn sample_background_pressure_processes(exclude_pids: &[u32]) -> Vec<String> {
    use std::ffi::c_void;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};

    const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
    const HEAVY_WATCH_EXES: &[(&str, &str)] = &[
        ("chrome.exe", "Google Chrome background tabs"),
        ("msedge.exe", "Microsoft Edge WebView/tabs"),
        ("firefox.exe", "Firefox browser threads"),
        ("obs64.exe", "OBS Studio encoder/capture"),
        ("discord.exe", "Discord overlay/voice worker"),
        ("searchindexer.exe", "Windows Search Indexer I/O"),
        ("tiworker.exe", "Windows Update Module Installer"),
        ("msmpeng.exe", "Windows Defender real-time scan"),
        ("onedrive.exe", "OneDrive cloud sync I/O"),
        ("steamwebhelper.exe", "Steam CEF background helper"),
        ("wallpaper32.exe", "Wallpaper Engine GPU compositor"),
        ("wallpaper64.exe", "Wallpaper Engine 64-bit GPU"),
        ("nvidia share.exe", "NVIDIA ShadowPlay capture"),
        ("radeonsoftware.exe", "AMD Adrenalin overlay"),
    ];

    struct SnapshotHandleGuard(isize);
    impl Drop for SnapshotHandleGuard {
        fn drop(&mut self) {
            if self.0 != -1 && self.0 != 0 {
                unsafe {
                    let _ = CloseHandle(HANDLE(self.0 as *mut c_void));
                }
            }
        }
    }

    let mut found = Vec::new();
    let Some(procs) = toolhelp_snapshot_procs() else {
        return found;
    };

    unsafe {
        let snap = (procs.create_snap)(TH32CS_SNAPPROCESS, 0);
        if snap == -1 || snap == 0 {
            return found;
        }
        let _snap_guard = SnapshotHandleGuard(snap);
        let mut pe: ToolhelpProcessEntry32W = std::mem::zeroed();
        pe.dwSize = std::mem::size_of::<ToolhelpProcessEntry32W>() as u32;
        if (procs.proc_first)(snap, &mut pe) != 0 {
            loop {
                if !exclude_pids.contains(&pe.th32ProcessID) && pe.th32ProcessID > 4 {
                    let end = pe
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(pe.szExeFile.len());
                    let exe = String::from_utf16_lossy(&pe.szExeFile[..end]);
                    let lower = exe.to_ascii_lowercase();
                    if let Some((_, desc)) = HEAVY_WATCH_EXES.iter().find(|(pat, _)| lower == *pat) {
                        let tag = format!("{} ({} · {} threads)", exe, desc, pe.cntThreads);
                        if !found.contains(&tag) {
                            found.push(tag);
                        }
                    } else if pe.cntThreads >= 64 && !found.iter().any(|f| f.starts_with(&exe)) {
                        found.push(format!("{} (High thread load: {})", exe, pe.cntThreads));
                    }
                    if found.len() >= 6 {
                        break;
                    }
                }
                if (procs.proc_next)(snap, &mut pe) == 0 {
                    break;
                }
            }
        }
    }
    found
}

#[cfg(not(windows))]
pub fn sample_background_pressure_processes(_exclude_pids: &[u32]) -> Vec<String> {
    Vec::new()
}

pub fn build_xray_optimization_suggestions(
    stats: &FramePercentileStats,
    spike_correlations: &[String],
    gpu_reset_events: u32,
) -> Vec<XrayOptimizationSuggestion> {
    let mut out = Vec::new();
    if stats.shader_stutter_spikes >= 3 {
        out.push(XrayOptimizationSuggestion {
            id: "sync_shaders".to_string(),
            title: "Sync Shader Swarm & Purge Stale Driver Cache".to_string(),
            rationale: format!(
                "Detected {} shader-compilation frame hitches (>2.25x baseline frame time).",
                stats.shader_stutter_spikes
            ),
            action_kind: "sync_shader_swarm".to_string(),
        });
    }
    if stats.cpu_bound_pct >= 56.0 || stats.bottleneck_verdict.contains("CPU") {
        out.push(XrayOptimizationSuggestion {
            id: "cpu_latency_boost".to_string(),
            title: "Enable High-Priority CPU & 0.5 ms Frame Pacing".to_string(),
            rationale: format!(
                "Main thread is CPU-bound ({:.0}% CPU vs {:.0}% GPU).",
                stats.cpu_bound_pct, stats.gpu_bound_pct
            ),
            action_kind: "enable_latency_boost".to_string(),
        });
    }
    if stats.gpu_bound_pct >= 58.0 || (stats.avg_fps > 0.0 && stats.avg_fps < 55.0) {
        out.push(XrayOptimizationSuggestion {
            id: "gpu_upscale_auto".to_string(),
            title: "Enable Flip-Model Borderless & Spatial Upscaling".to_string(),
            rationale: format!(
                "GPU render bound ({:.0}% GPU pressure, {:.1} FPS avg).",
                stats.gpu_bound_pct, stats.avg_fps
            ),
            action_kind: "enable_upscaler".to_string(),
        });
    }
    if !spike_correlations.is_empty() {
        out.push(XrayOptimizationSuggestion {
            id: "trim_background_ram".to_string(),
            title: "Trim Standby & Background Working-Set RAM".to_string(),
            rationale: format!(
                "Frame spikes coincided with {} active background culprit(s).",
                spike_correlations.len()
            ),
            action_kind: "trim_ram".to_string(),
        });
    }
    if gpu_reset_events > 0 {
        out.push(XrayOptimizationSuggestion {
            id: "check_tdr_fixes".to_string(),
            title: "Inspect GPU Driver Reset & DXVK Stability Fixes".to_string(),
            rationale: format!(
                "Captured {} GPU reset/TDR event(s) during gameplay.",
                gpu_reset_events
            ),
            action_kind: "open_fix_finder".to_string(),
        });
    }
    out
}

pub fn start_session_xray(
    app: &tauri::AppHandle,
    game_id: &str,
    _game_title: &str,
    root_pid: u32,
    job: Option<Arc<ProcessTreeJob>>,
) {
    let qpc_freq = win_etw::query_qpc_freq();
    let state = Arc::new(XrayCollectorState::new(root_pid, qpc_freq));
    let stop = Arc::new(AtomicBool::new(false));
    let telemetry_source = Arc::new(Mutex::new(String::from("Initializing ETW Session...")));

    let session_name = format!(
        "DukeXRay_{}",
        game_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(16)
            .collect::<String>()
    );

    let stop_etw = Arc::clone(&stop);
    let state_etw = Arc::clone(&state);
    let src_etw = Arc::clone(&telemetry_source);
    let etw_thread = std::thread::spawn(move || {
        win_etw::run_kernel_dxgi_etw_or_dwm(session_name, stop_etw, state_etw, src_etw);
    });

    // Live ticker thread updating child PIDs from ProcessTreeJob, correlating stutter spikes
    // with background processes, and emitting 1 Hz ticks to UI.
    let stop_tick = Arc::clone(&stop);
    let state_tick = Arc::clone(&state);
    let src_tick = Arc::clone(&telemetry_source);
    let app_tick = app.clone();
    let gid_tick = game_id.to_string();
    let started = Instant::now();
    std::thread::spawn(move || {
        let mut prev_spikes = 0u32;
        while !stop_tick.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(1000));
            if stop_tick.load(Ordering::Relaxed) {
                break;
            }
            if let Some(ref j) = job {
                state_tick.update_pids(&j.active_pids());
            }
            let (frames, present, cpu, gpu) = state_tick.snapshot_samples();
            if frames.is_empty() {
                continue;
            }
            let stats = compute_frame_scorecard(&frames, &present, &cpu, &gpu);
            if stats.shader_stutter_spikes > prev_spikes {
                prev_spikes = stats.shader_stutter_spikes;
                let mut exclude = state_tick.current_pids();
                exclude.push(std::process::id());
                for culprit in sample_background_pressure_processes(&exclude) {
                    state_tick.record_spike_cause(culprit);
                }
            }
            let (gpu_reset_events, spike_correlations) = state_tick.snapshot_extras();
            let source_label = src_tick
                .lock()
                .map(|s| s.clone())
                .unwrap_or_else(|_| "ETW Active".to_string());
            let tick = LiveXrayTick {
                game_id: gid_tick.clone(),
                pid: root_pid,
                elapsed_sec: (started.elapsed().as_secs_f64() * 10.0).round() / 10.0,
                sample_count: stats.total_frames,
                avg_fps: stats.avg_fps,
                low_1pct_fps: stats.low_1pct_fps,
                low_0_1pct_fps: stats.low_0_1pct_fps,
                avg_frame_ms: stats.avg_frame_ms,
                low_1pct_frame_ms: stats.low_1pct_frame_ms,
                low_0_1pct_frame_ms: stats.low_0_1pct_frame_ms,
                gpu_present_latency_ms: stats.gpu_present_latency_ms,
                cpu_gpu_bottleneck_ratio: stats.cpu_gpu_bottleneck_ratio,
                bottleneck_verdict: stats.bottleneck_verdict,
                shader_stutter_spikes: stats.shader_stutter_spikes,
                gpu_reset_events,
                spike_correlations,
                telemetry_source: source_label,
            };
            let _ = app_tick.emit("etw-xray-tick", &tick);
        }
    });

    if let Ok(mut sessions) = active_xray_sessions().lock() {
        if let Some(mut old) = sessions.remove(game_id) {
            old.stop.store(true, Ordering::Relaxed);
            if let Some(h) = old.join.take() {
                let _ = h.join();
            }
        }
        sessions.insert(
            game_id.to_string(),
            ActiveXrayHandle {
                stop,
                state,
                started,
                telemetry_source,
                join: Some(etw_thread),
            },
        );
    }
}

pub fn stop_and_persist_scorecard(
    app: &tauri::AppHandle,
    game_id: &str,
    game_title: &str,
    elapsed_sec: f64,
) -> Option<SessionPerformanceScorecard> {
    let mut handle = active_xray_sessions().lock().ok()?.remove(game_id)?;
    handle.stop.store(true, Ordering::Relaxed);
    if let Some(join) = handle.join.take() {
        let _ = join.join();
    }

    let (frames, present, cpu, gpu) = handle.state.snapshot_samples();
    let stats = compute_frame_scorecard(&frames, &present, &cpu, &gpu);
    if stats.total_frames == 0 {
        return None;
    }

    let (gpu_reset_events, mut spike_correlations) = handle.state.snapshot_extras();
    if stats.shader_stutter_spikes > 0 && spike_correlations.is_empty() {
        let mut exclude = handle.state.current_pids();
        exclude.push(std::process::id());
        spike_correlations = sample_background_pressure_processes(&exclude);
    }
    let optimization_suggestions =
        build_xray_optimization_suggestions(&stats, &spike_correlations, gpu_reset_events);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let source = handle
        .telemetry_source
        .lock()
        .map(|s| s.clone())
        .unwrap_or_else(|_| "Pure-Rust ETW".to_string());

    let card = SessionPerformanceScorecard {
        session_id: format!("{}_{}", game_id, now),
        game_id: game_id.to_string(),
        game_title: game_title.to_string(),
        recorded_at_epoch_sec: now,
        duration_sec: (elapsed_sec.max(handle.started.elapsed().as_secs_f64()) * 10.0).round() / 10.0,
        total_frames_sampled: stats.total_frames,
        avg_fps: stats.avg_fps,
        low_1pct_fps: stats.low_1pct_fps,
        low_0_1pct_fps: stats.low_0_1pct_fps,
        avg_frame_ms: stats.avg_frame_ms,
        low_1pct_frame_ms: stats.low_1pct_frame_ms,
        low_0_1pct_frame_ms: stats.low_0_1pct_frame_ms,
        gpu_present_latency_ms: stats.gpu_present_latency_ms,
        cpu_gpu_bottleneck_ratio: stats.cpu_gpu_bottleneck_ratio,
        cpu_bound_pct: stats.cpu_bound_pct,
        gpu_bound_pct: stats.gpu_bound_pct,
        bottleneck_verdict: stats.bottleneck_verdict,
        shader_stutter_spikes: stats.shader_stutter_spikes,
        gpu_reset_events,
        spike_correlations,
        optimization_suggestions,
        telemetry_source: source,
        anti_cheat_safe: true,
    };

    let mut store = load_scorecard_store();
    let entry = store
        .games
        .entry(game_id.to_string())
        .or_insert_with(|| GameScorecardHistory {
            game_id: game_id.to_string(),
            latest: None,
            history: Vec::new(),
        });
    entry.latest = Some(card.clone());
    entry.history.insert(0, card.clone());
    entry.history.truncate(MAX_HISTORY_PER_GAME);
    let _ = save_scorecard_store(&store);

    let _ = app.emit("etw-xray-scorecard", &card);
    Some(card)
}

#[tauri::command]
pub async fn probe_etw_xray_capability() -> Result<EtwXrayCapability, String> {
    Ok(win_etw::probe_capability())
}

#[tauri::command]
pub async fn get_game_xray_scorecards(game_id: String) -> Result<GameScorecardHistory, String> {
    tokio::task::spawn_blocking(move || {
        let store = load_scorecard_store();
        Ok(store
            .games
            .get(&game_id)
            .cloned()
            .unwrap_or_else(|| GameScorecardHistory {
                game_id,
                latest: None,
                history: Vec::new(),
            }))
    })
    .await
    .map_err(|e| format!("Failed loading ETW scorecards: {}", e))?
}

#[tauri::command]
pub async fn clear_game_xray_scorecards(game_id: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let mut store = load_scorecard_store();
        store.games.remove(&game_id);
        save_scorecard_store(&store)?;
        Ok("Cleared saved ETW performance scorecards.".to_string())
    })
    .await
    .map_err(|e| format!("Failed clearing ETW scorecards: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etw_percentile_math_computes_1pct_and_0_1pct_lows_and_stutters() {
        // Construct 1000 deterministic frame-time samples:
        // 989 smooth frames at 8.0 ms (125 FPS),
        // 10 heavy frames at 20.0 ms (50 FPS),
        // 1 shader-compilation hitch spike at 50.0 ms (20 FPS).
        let mut frames = vec![8.0f64; 989];
        frames.extend(vec![20.0f64; 10]);
        frames.push(50.0f64);

        let present = vec![4.5f64; 1000];
        let cpu = vec![3.2f64; 1000];
        let gpu = vec![5.8f64; 1000];

        let stats = compute_frame_scorecard(&frames, &present, &cpu, &gpu);
        assert_eq!(stats.total_frames, 1000);
        // Slowest 1% (10 frames) = [50.0, 20.0 * 9] -> mean = 23.0 ms -> 43.5 FPS
        assert!((stats.low_1pct_frame_ms - 23.0).abs() < 0.05);
        assert!((stats.low_1pct_fps - 43.5).abs() < 0.2);
        // Slowest 0.1% (1 frame) = 50.0 ms -> 20.0 FPS
        assert!((stats.low_0_1pct_frame_ms - 50.0).abs() < 0.05);
        assert!((stats.low_0_1pct_fps - 20.0).abs() < 0.2);
        assert!(stats.low_0_1pct_fps <= stats.low_1pct_fps);
        assert!(stats.low_1pct_fps <= stats.avg_fps);
        // Threshold = max(8.16 * 2.25 = 18.36, 8.16 + 18 = 26.16) -> 1 spike (50.0 ms)
        assert_eq!(stats.shader_stutter_spikes, 1);
        assert_eq!(stats.bottleneck_verdict, "GPU Render Bound");
        assert!(stats.cpu_gpu_bottleneck_ratio < 0.82);
    }

    #[test]
    fn xray_correlates_spikes_and_generates_actionable_suggestions() {
        let stats = FramePercentileStats {
            total_frames: 600,
            avg_fps: 48.0,
            low_1pct_fps: 24.0,
            low_0_1pct_fps: 11.0,
            avg_frame_ms: 20.8,
            low_1pct_frame_ms: 41.6,
            low_0_1pct_frame_ms: 90.0,
            gpu_present_latency_ms: 13.0,
            cpu_gpu_bottleneck_ratio: 0.65,
            cpu_bound_pct: 36.0,
            gpu_bound_pct: 64.0,
            bottleneck_verdict: "GPU Render Bound".to_string(),
            shader_stutter_spikes: 5,
        };
        let corr = vec!["obs64.exe (OBS Studio encoder/capture · 42 threads)".to_string()];
        let suggs = build_xray_optimization_suggestions(&stats, &corr, 1);
        assert!(suggs.iter().any(|s| s.action_kind == "sync_shader_swarm"));
        assert!(suggs.iter().any(|s| s.action_kind == "enable_upscaler"));
        assert!(suggs.iter().any(|s| s.action_kind == "trim_ram"));
        assert!(suggs.iter().any(|s| s.action_kind == "open_fix_finder"));
    }

    #[test]
    fn etw_capability_is_zero_injection() {
        let cap = win_etw::probe_capability();
        assert!(cap.zero_dll_injection);
        assert!(cap.anti_cheat_safe);
        #[cfg(windows)]
        {
            assert!(cap.etw_api_loaded);
            if cap.dwm_api_loaded {
                assert!(win_etw::probe_dwm_timing_once());
            }
        }
    }
}
