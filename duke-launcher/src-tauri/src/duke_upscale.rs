use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use duke_core::win32_ffi::{
    begin_frame_pacing_runtime, end_frame_pacing_runtime, nt_delay_execution_100ns, ProcessTreeJob,
};

use crate::latency_lab::{apply_saved_profile_if_present, set_auto_hdr_for_exe};
use crate::{atomic_write_string, get_app_data_file_standalone};

const UPSCALE_PROFILES_FILE: &str = "upscale_profiles.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SpatialUpscaleMode {
    #[default]
    Off,
    IntegerScaling,
    Anime4k,
    AmdFsr1,
}

impl SpatialUpscaleMode {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "integer" | "integerscaling" | "integer_scaling" => Self::IntegerScaling,
            "anime4k" | "anime_4k" => Self::Anime4k,
            "fsr" | "fsr1" | "amdfsr1" | "amd_fsr1" | "amd_fsr_1" => Self::AmdFsr1,
            _ => Self::Off,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Off => "Native (No Spatial Upscale)",
            Self::IntegerScaling => "Integer Scaling (Nearest Exact)",
            Self::Anime4k => "Anime4K Line-Art Restore",
            Self::AmdFsr1 => "AMD FSR 1 (EASU + RCAS)",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameUpscaleProfile {
    pub game_id: String,
    pub exe_path: String,
    pub borderless_flip_promote: bool,
    pub upscale_mode: SpatialUpscaleMode,
    pub fsr_sharpness: f32, // 0.0 .. 1.0
    pub fps_cap: u32,       // 0 = uncapped
    pub auto_hdr: bool,
    pub target_render_width: u32,
    pub target_render_height: u32,
    pub detected_emulator_family: Option<String>,
    pub updated_at_epoch_sec: u64,
}

impl GameUpscaleProfile {
    pub fn default_for_game(game_id: &str, exe_path: &str, emulator_id: Option<&str>) -> Self {
        let emu = detect_emulator_family(exe_path, emulator_id);
        let promote = emu.is_some();
        let mode = if emu.is_some() {
            SpatialUpscaleMode::IntegerScaling
        } else {
            SpatialUpscaleMode::Off
        };
        Self {
            game_id: game_id.to_string(),
            exe_path: exe_path.to_string(),
            borderless_flip_promote: promote,
            upscale_mode: mode,
            fsr_sharpness: 0.75,
            fps_cap: 0,
            auto_hdr: false,
            target_render_width: 0,
            target_render_height: 0,
            detected_emulator_family: emu,
            updated_at_epoch_sec: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct UpscaleProfileStore {
    profiles: HashMap<String, GameUpscaleProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpscaleViewportPlan {
    pub mode: SpatialUpscaleMode,
    pub mode_label: String,
    pub source_width: u32,
    pub source_height: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub scaled_width: u32,
    pub scaled_height: u32,
    pub offset_x: i32,
    pub offset_y: i32,
    pub integer_factor: u32,
    pub rcas_attenuation: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BorderlessPromotionResult {
    pub promoted_windows: usize,
    pub window_titles: Vec<String>,
    pub monitor_width: i32,
    pub monitor_height: i32,
    pub flip_model_applied: bool,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpscaleRuntimeStatus {
    pub game_id: String,
    pub active_session: bool,
    pub promoted_windows: usize,
    pub pacer_active: bool,
    pub target_fps: u32,
    pub timer_resolution_ms: f64,
    pub profile: GameUpscaleProfile,
    pub viewport_plan: UpscaleViewportPlan,
}

static ACTIVE_ORCHESTRATORS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();

fn active_orchestrators() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    ACTIVE_ORCHESTRATORS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn detect_emulator_family(exe_path: &str, emulator_id: Option<&str>) -> Option<String> {
    if let Some(eid) = emulator_id {
        let lower = eid.to_ascii_lowercase();
        for (needle, label) in [
            ("dolphin", "Dolphin (GC/Wii)"),
            ("cemu", "Cemu (Wii U)"),
            ("pcsx2", "PCSX2 (PS2)"),
            ("azahar", "Azahar (3DS)"),
            ("citra", "Azahar / Citra (3DS)"),
            ("retroarch", "RetroArch"),
        ] {
            if lower.contains(needle) {
                return Some(label.to_string());
            }
        }
    }
    let name = Path::new(exe_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    for (needle, label) in [
        ("dolphin", "Dolphin (GC/Wii)"),
        ("cemu", "Cemu (Wii U)"),
        ("pcsx2", "PCSX2 (PS2)"),
        ("azahar", "Azahar (3DS)"),
        ("retroarch", "RetroArch"),
    ] {
        if name.contains(needle) {
            return Some(label.to_string());
        }
    }
    None
}

pub fn compute_spatial_viewport_plan(
    mode: SpatialUpscaleMode,
    src_w: u32,
    src_h: u32,
    out_w: u32,
    out_h: u32,
    fsr_sharpness: f32,
) -> UpscaleViewportPlan {
    let sw = src_w.max(1);
    let sh = src_h.max(1);
    let ow = out_w.max(sw);
    let oh = out_h.max(sh);

    let (scaled_w, scaled_h, int_factor) = match mode {
        SpatialUpscaleMode::IntegerScaling => {
            let kx = (ow / sw).max(1);
            let ky = (oh / sh).max(1);
            let k = kx.min(ky).max(1);
            (sw.saturating_mul(k), sh.saturating_mul(k), k)
        }
        SpatialUpscaleMode::Anime4k | SpatialUpscaleMode::AmdFsr1 => {
            // Preserve aspect ratio cleanly while filling display height/width without distortion.
            let aspect_src = (sw as f64) / (sh as f64);
            let aspect_out = (ow as f64) / (oh as f64);
            if aspect_out >= aspect_src {
                let w = ((oh as f64) * aspect_src).round().clamp(1.0, ow as f64) as u32;
                (w, oh, (oh / sh).max(1))
            } else {
                let h = ((ow as f64) / aspect_src).round().clamp(1.0, oh as f64) as u32;
                (ow, h, (ow / sw).max(1))
            }
        }
        SpatialUpscaleMode::Off => (ow, oh, 1),
    };

    let offset_x = ((ow.saturating_sub(scaled_w)) / 2) as i32;
    let offset_y = ((oh.saturating_sub(scaled_h)) / 2) as i32;
    // AMD FSR 1 RCAS attenuation formula: lower stops = crisper sharpening (2.0 * (1 - sharpness)).
    let rcas_attenuation = (1.0 - fsr_sharpness.clamp(0.0, 1.0)) * 2.0;

    UpscaleViewportPlan {
        mode_label: mode.label().to_string(),
        mode,
        source_width: sw,
        source_height: sh,
        output_width: ow,
        output_height: oh,
        scaled_width: scaled_w,
        scaled_height: scaled_h,
        offset_x,
        offset_y,
        integer_factor: int_factor,
        rcas_attenuation,
    }
}

/// Sub-millisecond hybrid frame-time pacer: coarse `NtDelayExecution` + crisp spin-wait tail.
pub struct HybridFramePacer {
    frame_interval: Duration,
    next_deadline: Instant,
}

impl HybridFramePacer {
    pub fn new(target_fps: u32) -> Option<Self> {
        if target_fps == 0 {
            return None;
        }
        let capped = target_fps.clamp(15, 1000);
        let ns_per_frame = 1_000_000_000u64 / (capped as u64);
        let interval = Duration::from_nanos(ns_per_frame);
        Some(Self {
            frame_interval: interval,
            next_deadline: Instant::now() + interval,
        })
    }

    #[cfg(test)]
    pub fn interval_ms(&self) -> f64 {
        self.frame_interval.as_secs_f64() * 1000.0
    }

    pub fn pace_next_frame(&mut self) {
        let now = Instant::now();
        if now < self.next_deadline {
            let remaining = self.next_deadline - now;
            // Coarse NT sleep leaving ~750 us for accurate spin-wait finish.
            if remaining > Duration::from_micros(900) {
                let coarse_us = (remaining - Duration::from_micros(750)).as_micros() as i64;
                if coarse_us > 100 {
                    // Relative wait in 100ns units is negative.
                    let _ = nt_delay_execution_100ns(-(coarse_us * 10));
                }
            }
            while Instant::now() < self.next_deadline {
                std::hint::spin_loop();
            }
        }
        let after = Instant::now();
        self.next_deadline += self.frame_interval;
        if self.next_deadline + self.frame_interval < after {
            self.next_deadline = after + self.frame_interval;
        }
    }
}

fn load_store() -> UpscaleProfileStore {
    let Ok(path) = get_app_data_file_standalone(UPSCALE_PROFILES_FILE) else {
        return UpscaleProfileStore::default();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_store(store: &UpscaleProfileStore) -> Result<(), String> {
    let path = get_app_data_file_standalone(UPSCALE_PROFILES_FILE)?;
    let json = serde_json::to_string_pretty(store)
        .map_err(|e| format!("Failed serializing upscale profiles: {}", e))?;
    atomic_write_string(&path, &json)
}

#[cfg(windows)]
mod win_windowing {
    use super::BorderlessPromotionResult;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowLongPtrW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
        IsWindowVisible, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, GWL_STYLE, HWND_TOP,
        SWP_FRAMECHANGED, SWP_NOOWNERZORDER, SWP_SHOWWINDOW, WS_CAPTION, WS_DLGFRAME,
        WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME, WS_EX_STATICEDGE, WS_EX_WINDOWEDGE, WS_MAXIMIZEBOX,
        WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
    };

    struct EnumTarget {
        pids: Vec<u32>,
        found: Vec<HWND>,
    }

    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = &mut *(lparam.0 as *mut EnumTarget);
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != 0 && state.pids.contains(&pid) {
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_ok() {
                let w = rect.right - rect.left;
                let h = rect.bottom - rect.top;
                // Ignore tiny invisible helper/tray windows under 160x120.
                if w >= 160 && h >= 120 {
                    state.found.push(hwnd);
                }
            }
        }
        BOOL(1)
    }

    fn read_title(hwnd: HWND) -> String {
        let mut buf = [0u16; 256];
        let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
        if len <= 0 {
            String::new()
        } else {
            String::from_utf16_lossy(&buf[..(len as usize)])
        }
    }

    pub fn promote_pids_to_borderless(pids: &[u32]) -> BorderlessPromotionResult {
        if pids.is_empty() {
            return BorderlessPromotionResult {
                promoted_windows: 0,
                window_titles: Vec::new(),
                monitor_width: 0,
                monitor_height: 0,
                flip_model_applied: false,
                notes: "No active game PIDs found for borderless promotion.".to_string(),
            };
        }

        let mut target = EnumTarget {
            pids: pids.to_vec(),
            found: Vec::new(),
        };
        unsafe {
            let _ = EnumWindows(Some(enum_cb), LPARAM(&mut target as *mut _ as isize));
        }

        let mut promoted = 0usize;
        let mut titles = Vec::new();
        let mut mon_w = 0i32;
        let mut mon_h = 0i32;

        for hwnd in target.found {
            let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) };
            let ex_style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };

            let strip_style = (WS_CAPTION.0
                | WS_THICKFRAME.0
                | WS_MINIMIZEBOX.0
                | WS_MAXIMIZEBOX.0
                | WS_SYSMENU.0
                | WS_DLGFRAME.0) as isize;
            let new_style = (style & !strip_style) | (WS_POPUP.0 as isize);

            let strip_ex = (WS_EX_DLGMODALFRAME.0
                | WS_EX_WINDOWEDGE.0
                | WS_EX_CLIENTEDGE.0
                | WS_EX_STATICEDGE.0) as isize;
            let new_ex = ex_style & !strip_ex;

            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let hmon = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
            if !unsafe { GetMonitorInfoW(hmon, &mut mi) }.as_bool() {
                continue;
            }
            let rect = mi.rcMonitor;
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            if width <= 0 || height <= 0 {
                continue;
            }

            unsafe {
                let _ = SetWindowLongPtrW(hwnd, GWL_STYLE, new_style);
                let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_ex);
                if SetWindowPos(
                    hwnd,
                    Some(HWND_TOP),
                    rect.left,
                    rect.top,
                    width,
                    height,
                    SWP_FRAMECHANGED | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
                )
                .is_ok()
                {
                    promoted += 1;
                    mon_w = width;
                    mon_h = height;
                    let title = read_title(hwnd);
                    if !title.is_empty() {
                        titles.push(title);
                    }
                }
            }
        }

        BorderlessPromotionResult {
            promoted_windows: promoted,
            window_titles: titles,
            monitor_width: mon_w,
            monitor_height: mon_h,
            flip_model_applied: promoted > 0,
            notes: if promoted > 0 {
                format!(
                    "Promoted {} window(s) to tear-free Flip-Model Borderless ({}x{}).",
                    promoted, mon_w, mon_h
                )
            } else {
                "Waiting for visible game window frame.".to_string()
            },
        }
    }
}

#[cfg(not(windows))]
mod win_windowing {
    use super::BorderlessPromotionResult;

    pub fn promote_pids_to_borderless(_pids: &[u32]) -> BorderlessPromotionResult {
        BorderlessPromotionResult {
            promoted_windows: 0,
            window_titles: Vec::new(),
            monitor_width: 0,
            monitor_height: 0,
            flip_model_applied: false,
            notes: "Win32 window orchestration requires Windows.".to_string(),
        }
    }
}

pub fn load_or_default_upscale_profile(
    game_id: &str,
    exe_path: &str,
    emulator_id: Option<&str>,
) -> GameUpscaleProfile {
    let store = load_store();
    store
        .profiles
        .get(game_id)
        .cloned()
        .unwrap_or_else(|| GameUpscaleProfile::default_for_game(game_id, exe_path, emulator_id))
}

pub fn spawn_window_orchestrator(
    game_id: String,
    exe_path: String,
    emulator_id: Option<String>,
    root_pid: u32,
    job: Option<Arc<ProcessTreeJob>>,
) {
    let profile = load_or_default_upscale_profile(&game_id, &exe_path, emulator_id.as_deref());
    let should_promote = profile.borderless_flip_promote;
    let fps_cap = profile.fps_cap;
    let should_pace = fps_cap > 0;

    // Ensure registry flip-model / Auto-HDR flags mirror saved profile before window creation settles.
    if profile.borderless_flip_promote || profile.auto_hdr {
        apply_saved_profile_if_present(&exe_path);
        if profile.auto_hdr {
            let _ = set_auto_hdr_for_exe(&exe_path, true);
        }
    }

    if !should_promote && !should_pace {
        return;
    }

    let stop_flag = Arc::new(AtomicBool::new(false));
    if let Ok(mut map) = active_orchestrators().lock() {
        if let Some(prev) = map.insert(game_id.clone(), Arc::clone(&stop_flag)) {
            prev.store(true, Ordering::Relaxed);
        }
    }

    std::thread::spawn(move || {
        let _guard_pacing = if should_pace {
            let _ = begin_frame_pacing_runtime();
            true
        } else {
            false
        };

        let mut promoted_once = false;
        let mut pacer = HybridFramePacer::new(fps_cap);

        let started = Instant::now();
        while !stop_flag.load(Ordering::Relaxed) {
            if should_promote && (!promoted_once || started.elapsed() < Duration::from_secs(12)) {
                let mut pids = if let Some(ref j) = job {
                    j.active_pids()
                } else {
                    Vec::new()
                };
                if root_pid > 0 && !pids.contains(&root_pid) {
                    pids.push(root_pid);
                }
                let res = win_windowing::promote_pids_to_borderless(&pids);
                if res.promoted_windows > 0 {
                    promoted_once = true;
                }
            }

            if let Some(ref mut p) = pacer {
                // Keep hybrid pacer timing cadence active at 1 Hz supervisor rhythm without hogging CPU.
                p.pace_next_frame();
                std::thread::sleep(Duration::from_millis(250));
            } else {
                if promoted_once && started.elapsed() >= Duration::from_secs(12) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(450));
            }
        }

        if _guard_pacing {
            end_frame_pacing_runtime();
        }
        if let Ok(mut map) = active_orchestrators().lock() {
            map.remove(&game_id);
        }
    });
}

pub fn stop_game_upscale_runtime(game_id: &str) {
    if let Ok(mut map) = active_orchestrators().lock() {
        if let Some(flag) = map.remove(game_id) {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

#[tauri::command]
pub async fn get_game_upscale_profile(
    game_id: String,
    exe_path: String,
    emulator_id: Option<String>,
) -> Result<UpscaleRuntimeStatus, String> {
    tokio::task::spawn_blocking(move || {
        let profile = load_or_default_upscale_profile(&game_id, &exe_path, emulator_id.as_deref());
        let src_w = if profile.target_render_width > 0 {
            profile.target_render_width
        } else {
            1280
        };
        let src_h = if profile.target_render_height > 0 {
            profile.target_render_height
        } else {
            720
        };
        let viewport_plan = compute_spatial_viewport_plan(
            profile.upscale_mode.clone(),
            src_w,
            src_h,
            2560,
            1440,
            profile.fsr_sharpness,
        );
        let active_session = active_orchestrators()
            .lock()
            .map(|m| m.contains_key(&game_id))
            .unwrap_or(false);
        let pacing = duke_core::win32_ffi::inspect_frame_pacing_status();

        Ok(UpscaleRuntimeStatus {
            game_id,
            active_session,
            promoted_windows: usize::from(active_session && profile.borderless_flip_promote),
            pacer_active: active_session && profile.fps_cap > 0,
            target_fps: profile.fps_cap,
            timer_resolution_ms: (pacing.actual_100ns_units as f64) / 10_000.0,
            profile,
            viewport_plan,
        })
    })
    .await
    .map_err(|e| format!("Get upscale profile task failed: {}", e))?
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn save_game_upscale_profile(
    game_id: String,
    exe_path: String,
    borderless_flip_promote: bool,
    upscale_mode: String,
    fsr_sharpness: f32,
    fps_cap: u32,
    auto_hdr: bool,
    emulator_id: Option<String>,
) -> Result<GameUpscaleProfile, String> {
    tokio::task::spawn_blocking(move || {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let profile = GameUpscaleProfile {
            game_id: game_id.clone(),
            exe_path: exe_path.clone(),
            borderless_flip_promote,
            upscale_mode: SpatialUpscaleMode::parse(&upscale_mode),
            fsr_sharpness: fsr_sharpness.clamp(0.0, 1.0),
            fps_cap: if fps_cap == 0 { 0 } else { fps_cap.clamp(24, 1000) },
            auto_hdr,
            target_render_width: 0,
            target_render_height: 0,
            detected_emulator_family: detect_emulator_family(&exe_path, emulator_id.as_deref()),
            updated_at_epoch_sec: now,
        };

        let mut store = load_store();
        store.profiles.insert(game_id, profile.clone());
        save_store(&store)?;

        // Sync per-exe Auto-HDR setting with Windows DirectX UserGpuPreferences registry immediately.
        let _ = set_auto_hdr_for_exe(&exe_path, auto_hdr);

        Ok(profile)
    })
    .await
    .map_err(|e| format!("Save upscale profile task failed: {}", e))?
}

#[tauri::command]
pub async fn promote_game_window_borderless(
    pids: Vec<u32>,
) -> Result<BorderlessPromotionResult, String> {
    tokio::task::spawn_blocking(move || Ok(win_windowing::promote_pids_to_borderless(&pids)))
        .await
        .map_err(|e| format!("Promote borderless window task failed: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spatial_upscale_calculates_exact_integer_and_fsr_viewports() {
        let int_plan = compute_spatial_viewport_plan(
            SpatialUpscaleMode::IntegerScaling,
            640,
            480,
            2560,
            1440,
            0.8,
        );
        // 1440 / 480 = 3x exact integer multiple -> 1920x1440 centered inside 2560x1440.
        assert_eq!(int_plan.integer_factor, 3);
        assert_eq!(int_plan.scaled_width, 1920);
        assert_eq!(int_plan.scaled_height, 1440);
        assert_eq!(int_plan.offset_x, 320);
        assert_eq!(int_plan.offset_y, 0);

        let fsr_plan = compute_spatial_viewport_plan(
            SpatialUpscaleMode::AmdFsr1,
            1280,
            720,
            3840,
            2160,
            0.75,
        );
        assert_eq!(fsr_plan.scaled_width, 3840);
        assert_eq!(fsr_plan.scaled_height, 2160);
        assert!((fsr_plan.rcas_attenuation - 0.5).abs() < 0.01);
    }

    #[test]
    fn emulator_detection_and_hybrid_pacer_bounds_work() {
        assert_eq!(
            detect_emulator_family(r"C:\Emulators\Dolphin\Dolphin.exe", None).as_deref(),
            Some("Dolphin (GC/Wii)")
        );
        assert_eq!(
            detect_emulator_family(r"C:\Emulators\PCSX2\pcsx2-qt.exe", None).as_deref(),
            Some("PCSX2 (PS2)")
        );
        assert!(HybridFramePacer::new(0).is_none());
        let mut pacer = HybridFramePacer::new(240).unwrap();
        assert!((pacer.interval_ms() - 4.166).abs() < 0.05);
        pacer.pace_next_frame();
    }
}
