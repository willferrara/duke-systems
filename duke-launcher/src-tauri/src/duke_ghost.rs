//! Deterministic Input Ghost & Macro Recorder (`duke_ghost.rs`).
//!
//! Records ultra-compact (`~10 KB/min`) delta-encoded Zstd `.dkghost` controller streams
//! synchronized with `Duke Time-Machine` save states for speedrun practice, shareable ghosts,
//! and macro loops. Bridges live playback frames to `duke-input` over the Duke IPC pipe bus
//! and **automatically pauses playback whenever the target game loses window focus**.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

use crate::{
    atomic_write_string, format_bytes, load_library_standalone, resolve_data_root_dir_standalone,
};

const GHOST_MAGIC: &[u8; 8] = b"DKGHST01";
const GHOST_FORMAT_VERSION: u8 = 1;
const GHOSTS_SUBDIR: &str = "ghosts";
const MAX_GHOST_BYTES: usize = 32 * 1024 * 1024;
const SAMPLE_INTERVAL_MS: u64 = 16; // ~60 Hz deterministic delta capture

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct GhostControllerSample {
    pub buttons: u16,
    pub lt: u8,
    pub rt: u8,
    pub lx: i8,
    pub ly: i8,
    pub rx: i8,
    pub ry: i8,
}

impl GhostControllerSample {
    /// Applies small analog stick jitter quantization so resting controllers emit zero delta frames.
    pub fn quantized(self) -> Self {
        let q_axis = |v: i8| -> i8 {
            if v.abs() <= 4 {
                0
            } else {
                (v / 4) * 4
            }
        };
        let q_trig = |v: u8| -> u8 {
            if v <= 4 {
                0
            } else {
                (v / 4) * 4
            }
        };
        Self {
            buttons: self.buttons,
            lt: q_trig(self.lt),
            rt: q_trig(self.rt),
            lx: q_axis(self.lx),
            ly: q_axis(self.ly),
            rx: q_axis(self.rx),
            ry: q_axis(self.ry),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GhostDeltaFrame {
    pub delta_ms: u16,
    pub state: GhostControllerSample,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GhostHeaderMetadata {
    pub ghost_id: String,
    pub game_id: String,
    pub game_title: String,
    pub target_exe_name: String,
    pub label: String,
    pub created_at_epoch_sec: u64,
    pub time_machine_checkpoint_id: Option<String>,
    pub duration_ms: u64,
    pub frame_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GhostRecordingSummary {
    pub ghost_id: String,
    pub game_id: String,
    pub game_title: String,
    pub target_exe_name: String,
    pub label: String,
    pub created_at_epoch_sec: u64,
    pub time_machine_checkpoint_id: Option<String>,
    pub duration_ms: u64,
    pub frame_count: usize,
    pub file_path: String,
    pub file_size_bytes: u64,
    pub file_size_formatted: String,
    pub kb_per_minute: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct GhostSessionStatus {
    pub game_id: String,
    pub mode: String, // "idle" | "recording" | "playing"
    pub active_ghost_id: Option<String>,
    pub paused_focus_lost: bool,
    pub loop_mode: bool,
    pub elapsed_ms: u64,
    pub total_ms: u64,
    pub loop_count: u32,
    pub frames_processed: usize,
    pub target_exe_name: String,
    pub time_machine_checkpoint_id: Option<String>,
    pub status_text: String,
}

/// Deterministic playback clock step helper ensuring elapsed time freezes immediately
/// whenever target game window focus is lost (`paused_focus_lost == true`).
#[derive(Debug, Clone)]
pub struct GhostFocusMachine {
    pub paused_focus_lost: bool,
    pub elapsed_ms: u64,
    pub loop_count: u32,
}

impl GhostFocusMachine {
    pub fn new() -> Self {
        Self {
            paused_focus_lost: false,
            elapsed_ms: 0,
            loop_count: 0,
        }
    }

    /// Advances playback clock strictly when `target_focused == true`.
    /// Returns `(advanced, wrapped_loop)`.
    pub fn advance(&mut self, target_focused: bool, dt_ms: u64, total_ms: u64, loop_mode: bool) -> (bool, bool) {
        if !target_focused {
            self.paused_focus_lost = true;
            return (false, false);
        }
        self.paused_focus_lost = false;
        self.elapsed_ms = self.elapsed_ms.saturating_add(dt_ms);
        if total_ms > 0 && self.elapsed_ms >= total_ms {
            if loop_mode {
                self.elapsed_ms %= total_ms.max(1);
                self.loop_count = self.loop_count.saturating_add(1);
                return (true, true);
            }
            self.elapsed_ms = total_ms;
            return (false, false);
        }
        (true, false)
    }
}

struct ActiveRecorderHandle {
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<Result<GhostRecordingSummary, String>>>,
}

struct ActivePlaybackHandle {
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

fn ghost_sessions_status() -> &'static Mutex<HashMap<String, GhostSessionStatus>> {
    static STATUS: OnceLock<Mutex<HashMap<String, GhostSessionStatus>>> = OnceLock::new();
    STATUS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn active_recorders() -> &'static Mutex<HashMap<String, ActiveRecorderHandle>> {
    static REC: OnceLock<Mutex<HashMap<String, ActiveRecorderHandle>>> = OnceLock::new();
    REC.get_or_init(|| Mutex::new(HashMap::new()))
}

fn active_playbacks() -> &'static Mutex<HashMap<String, ActivePlaybackHandle>> {
    static PLAY: OnceLock<Mutex<HashMap<String, ActivePlaybackHandle>>> = OnceLock::new();
    PLAY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn update_status(game_id: &str, status: GhostSessionStatus) {
    if let Ok(mut map) = ghost_sessions_status().lock() {
        map.insert(game_id.to_string(), status);
    }
}

fn ghost_game_dir(game_id: &str) -> Result<PathBuf, String> {
    let safe: String = game_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(64)
        .collect();
    let dir = resolve_data_root_dir_standalone()?
        .join(GHOSTS_SUBDIR)
        .join(if safe.is_empty() { "default" } else { &safe });
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed creating ghost directory '{}': {}", dir.display(), e))?;
    Ok(dir)
}

/// Encodes delta controller frames into a `.dkghost` binary payload:
/// `DKGHST01` (8B) | version (1B) | metadata_len (u32 LE) | metadata UTF-8 JSON | Zstd(10B/frame LE array).
pub fn encode_dkghost_bytes(
    meta: &GhostHeaderMetadata,
    frames: &[GhostDeltaFrame],
) -> Result<Vec<u8>, String> {
    let meta_json = serde_json::to_vec(meta)
        .map_err(|e| format!("Failed serializing ghost metadata: {e}"))?;
    let mut raw = Vec::with_capacity(frames.len().saturating_mul(10));
    for f in frames {
        raw.extend_from_slice(&f.delta_ms.to_le_bytes());
        raw.extend_from_slice(&f.state.buttons.to_le_bytes());
        raw.push(f.state.lt);
        raw.push(f.state.rt);
        raw.push(f.state.lx as u8);
        raw.push(f.state.ly as u8);
        raw.push(f.state.rx as u8);
        raw.push(f.state.ry as u8);
    }
    let compressed = zstd::encode_all(raw.as_slice(), 3)
        .map_err(|e| format!("Failed compressing ghost stream: {e}"))?;

    let mut out = Vec::with_capacity(13 + meta_json.len() + compressed.len());
    out.extend_from_slice(GHOST_MAGIC);
    out.push(GHOST_FORMAT_VERSION);
    out.extend_from_slice(&(meta_json.len() as u32).to_le_bytes());
    out.extend_from_slice(&meta_json);
    out.extend_from_slice(&compressed);
    Ok(out)
}

pub fn decode_dkghost_bytes(bytes: &[u8]) -> Result<(GhostHeaderMetadata, Vec<GhostDeltaFrame>), String> {
    if bytes.len() < 13 {
        return Err("Corrupt or truncated .dkghost file.".to_string());
    }
    if &bytes[..8] != GHOST_MAGIC {
        return Err("Invalid .dkghost magic header.".to_string());
    }
    if bytes[8] != GHOST_FORMAT_VERSION {
        return Err(format!("Unsupported .dkghost version: {}", bytes[8]));
    }
    let meta_len = u32::from_le_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]) as usize;
    let meta_end = 13usize.saturating_add(meta_len);
    if meta_end > bytes.len() || meta_len > 256 * 1024 {
        return Err("Invalid .dkghost metadata header size.".to_string());
    }
    let meta: GhostHeaderMetadata = serde_json::from_slice(&bytes[13..meta_end])
        .map_err(|e| format!("Corrupt .dkghost metadata JSON: {e}"))?;

    let mut decoder = zstd::Decoder::new(&bytes[meta_end..])
        .map_err(|e| format!("Failed initializing Zstd ghost decoder: {e}"))?;
    let mut raw = Vec::new();
    decoder
        .by_ref()
        .take(MAX_GHOST_BYTES as u64)
        .read_to_end(&mut raw)
        .map_err(|e| format!("Failed decoding Zstd ghost stream: {e}"))?;
    if !raw.len().is_multiple_of(10) {
        return Err("Corrupt .dkghost frame alignment.".to_string());
    }
    let mut frames = Vec::with_capacity(raw.len() / 10);
    for chunk in raw.as_chunks::<10>().0 {
        let delta_ms = u16::from_le_bytes([chunk[0], chunk[1]]);
        let buttons = u16::from_le_bytes([chunk[2], chunk[3]]);
        frames.push(GhostDeltaFrame {
            delta_ms,
            state: GhostControllerSample {
                buttons,
                lt: chunk[4],
                rt: chunk[5],
                lx: chunk[6] as i8,
                ly: chunk[7] as i8,
                rx: chunk[8] as i8,
                ry: chunk[9] as i8,
            },
        });
    }
    Ok((meta, frames))
}

fn read_ghost_summary(path: &Path) -> Result<GhostRecordingSummary, String> {
    let data = fs::read(path)
        .map_err(|e| format!("Failed reading ghost file '{}': {}", path.display(), e))?;
    let (meta, _) = decode_dkghost_bytes(&data)?;
    let size = data.len() as u64;
    let mins = (meta.duration_ms as f64 / 60_000.0).max(1e-4);
    let kb_per_minute = ((size as f64 / 1024.0) / mins * 10.0).round() / 10.0;
    Ok(GhostRecordingSummary {
        ghost_id: meta.ghost_id,
        game_id: meta.game_id,
        game_title: meta.game_title,
        target_exe_name: meta.target_exe_name,
        label: meta.label,
        created_at_epoch_sec: meta.created_at_epoch_sec,
        time_machine_checkpoint_id: meta.time_machine_checkpoint_id,
        duration_ms: meta.duration_ms,
        frame_count: meta.frame_count,
        file_path: path.to_string_lossy().into_owned(),
        file_size_bytes: size,
        file_size_formatted: format_bytes(size),
        kb_per_minute,
    })
}

/// Polls XInput port 0 dynamically without bloating dependencies.
#[cfg(windows)]
fn poll_primary_xinput_sample() -> GhostControllerSample {
    use windows::core::{PCSTR, PCWSTR};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct XInputGamepadRaw {
        w_buttons: u16,
        b_left_trigger: u8,
        b_right_trigger: u8,
        s_thumb_lx: i16,
        s_thumb_ly: i16,
        s_thumb_rx: i16,
        s_thumb_ry: i16,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct XInputStateRaw {
        dw_packet_number: u32,
        gamepad: XInputGamepadRaw,
    }
    type XInputGetStateFn = unsafe extern "system" fn(u32, *mut XInputStateRaw) -> u32;

    static FN_PTR: OnceLock<Option<XInputGetStateFn>> = OnceLock::new();
    let get_state = *FN_PTR.get_or_init(|| unsafe {
        for dll in ["xinput1_4.dll\0", "xinput9_1_0.dll\0", "xinput1_3.dll\0"] {
            let wide: Vec<u16> = dll.encode_utf16().collect();
            if let Ok(h) = LoadLibraryW(PCWSTR(wide.as_ptr())) {
                if !h.is_invalid() {
                    if let Some(p) = GetProcAddress(h, PCSTR(c"XInputGetState".as_ptr().cast())) {
                        return Some(std::mem::transmute::<
                            unsafe extern "system" fn() -> isize,
                            XInputGetStateFn,
                        >(p));
                    }
                }
            }
        }
        None
    });

    let Some(fn_get) = get_state else {
        return GhostControllerSample::default();
    };
    let mut st: XInputStateRaw = unsafe { std::mem::zeroed() };
    let rc = unsafe { fn_get(0, &mut st) };
    if rc != 0 {
        return GhostControllerSample::default();
    }
    let scale_i8 = |v: i16| -> i8 { ((v as i32) / 258).clamp(-127, 127) as i8 };
    GhostControllerSample {
        buttons: st.gamepad.w_buttons,
        lt: st.gamepad.b_left_trigger,
        rt: st.gamepad.b_right_trigger,
        lx: scale_i8(st.gamepad.s_thumb_lx),
        ly: scale_i8(st.gamepad.s_thumb_ly),
        rx: scale_i8(st.gamepad.s_thumb_rx),
        ry: scale_i8(st.gamepad.s_thumb_ry),
    }
    .quantized()
}

#[cfg(not(windows))]
fn poll_primary_xinput_sample() -> GhostControllerSample {
    GhostControllerSample::default()
}

#[cfg(windows)]
fn get_foreground_exe_name_lower() -> String {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return String::new();
        }
        let mut pid: u32 = 0;
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return String::new();
        }
        let Ok(hproc) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        if hproc.is_invalid() {
            return String::new();
        }
        let mut buf = [0u16; 260];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            hproc,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(hproc);
        if !ok || len == 0 {
            return String::new();
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        full.rsplit(['\\', '/'])
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase()
    }
}

/// Checks whether the target game executable (or active PID) owns the foreground HWND.
#[cfg(windows)]
fn is_target_game_focused(target_exe_name: &str) -> bool {
    let fg = get_foreground_exe_name_lower();
    if fg.is_empty() {
        return false;
    }
    let want = target_exe_name.trim().to_ascii_lowercase();
    if want.is_empty() {
        return true;
    }
    fg == want
}

#[cfg(not(windows))]
fn is_target_game_focused(_target_exe_name: &str) -> bool {
    true
}

fn resolve_game_meta(game_id: &str) -> Result<(String, String), String> {
    let library = load_library_standalone()?;
    let g = library
        .iter()
        .find(|e| e.id == game_id)
        .ok_or_else(|| format!("Game '{}' not found in library.", game_id))?;
    let exe_name = Path::new(&g.exe_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    Ok((g.title.clone(), exe_name))
}

#[tauri::command]
pub async fn list_game_ghosts(game_id: String) -> Result<Vec<GhostRecordingSummary>, String> {
    tokio::task::spawn_blocking(move || {
        let dir = ghost_game_dir(&game_id)?;
        let mut out = Vec::new();
        if let Ok(rd) = fs::read_dir(&dir) {
            for ent in rd.flatten() {
                let p = ent.path();
                if p.extension().and_then(|s| s.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("dkghost")) {
                    if let Ok(sum) = read_ghost_summary(&p) {
                        out.push(sum);
                    }
                }
            }
        }
        out.sort_by_key(|a| std::cmp::Reverse(a.created_at_epoch_sec));
        Ok(out)
    })
    .await
    .map_err(|e| format!("Failed listing game ghosts: {e}"))?
}

#[tauri::command]
pub async fn get_ghost_status(game_id: String) -> Result<GhostSessionStatus, String> {
    let status = ghost_sessions_status()
        .lock()
        .ok()
        .and_then(|m| m.get(&game_id).cloned())
        .unwrap_or_else(|| GhostSessionStatus {
            game_id,
            mode: "idle".to_string(),
            status_text: "Ready to record ~10 KB/min input ghost.".to_string(),
            ..Default::default()
        });
    Ok(status)
}

#[tauri::command]
pub async fn start_ghost_recording(
    app: AppHandle,
    game_id: String,
    label: Option<String>,
    sync_time_machine: Option<bool>,
) -> Result<GhostSessionStatus, String> {
    let _ = stop_ghost_playback_internal(&game_id);
    if let Ok(mut recs) = active_recorders().lock() {
        if let Some(mut prev) = recs.remove(&game_id) {
            prev.stop.store(true, Ordering::Relaxed);
            if let Some(j) = prev.join.take() {
                let _ = j.join();
            }
        }
    }

    let (game_title, target_exe_name) = resolve_game_meta(&game_id)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let clean_label = label
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("Ghost Run #{now}"));

    let time_machine_cp_id = if sync_time_machine.unwrap_or(true) {
        crate::time_machine::create_time_machine_checkpoint(
            app.clone(),
            game_id.clone(),
            Some(format!("Ghost Sync: {}", clean_label)),
            Some("ghost_sync".to_string()),
        )
        .await
        .ok()
        .map(|cp| cp.id)
    } else {
        None
    };

    let ghost_id = format!("ghost_{}", now);
    let out_dir = ghost_game_dir(&game_id)?;
    let out_path = out_dir.join(format!("{}.dkghost", ghost_id));

    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = Arc::clone(&stop);
    let gid = game_id.clone();
    let gtitle = game_title.clone();
    let gexe = target_exe_name.clone();
    let glabel = clean_label.clone();
    let gid_id = ghost_id.clone();
    let tm_id = time_machine_cp_id.clone();
    let app_clone = app.clone();

    let initial_status = GhostSessionStatus {
        game_id: game_id.clone(),
        mode: "recording".to_string(),
        active_ghost_id: Some(ghost_id.clone()),
        paused_focus_lost: false,
        loop_mode: false,
        elapsed_ms: 0,
        total_ms: 0,
        loop_count: 0,
        frames_processed: 0,
        target_exe_name: target_exe_name.clone(),
        time_machine_checkpoint_id: time_machine_cp_id.clone(),
        status_text: format!(
            "Recording '{}' ({})...",
            clean_label,
            if time_machine_cp_id.is_some() {
                "Time-Machine synced"
            } else {
                "Controller stream"
            }
        ),
    };
    update_status(&game_id, initial_status.clone());
    let _ = app.emit("ghost-session-status", &initial_status);

    let handle = std::thread::spawn(move || {
        let mut frames: Vec<GhostDeltaFrame> = Vec::with_capacity(1024);
        let mut prev_state = GhostControllerSample::default();
        let mut total_elapsed_ms = 0u64;
        let mut delta_acc_ms = 0u64;
        let mut last_emit = Instant::now();
        let mut first_sample = true;

        while !stop_flag.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(SAMPLE_INTERVAL_MS));
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }
            let focused = is_target_game_focused(&gexe);
            if !focused && !first_sample {
                if last_emit.elapsed() >= Duration::from_millis(250) {
                    last_emit = Instant::now();
                    let st = GhostSessionStatus {
                        game_id: gid.clone(),
                        mode: "recording".to_string(),
                        active_ghost_id: Some(gid_id.clone()),
                        paused_focus_lost: true,
                        loop_mode: false,
                        elapsed_ms: total_elapsed_ms,
                        total_ms: total_elapsed_ms,
                        loop_count: 0,
                        frames_processed: frames.len(),
                        target_exe_name: gexe.clone(),
                        time_machine_checkpoint_id: tm_id.clone(),
                        status_text: format!("Paused (focus left {})", gexe),
                    };
                    update_status(&gid, st.clone());
                    let _ = app_clone.emit("ghost-session-status", &st);
                }
                continue;
            }

            let cur = poll_primary_xinput_sample();
            delta_acc_ms = delta_acc_ms.saturating_add(SAMPLE_INTERVAL_MS);
            total_elapsed_ms = total_elapsed_ms.saturating_add(SAMPLE_INTERVAL_MS);

            if first_sample || cur != prev_state || delta_acc_ms >= 500 {
                let d_ms = delta_acc_ms.min(u16::MAX as u64) as u16;
                frames.push(GhostDeltaFrame {
                    delta_ms: if first_sample { 0 } else { d_ms },
                    state: cur,
                });
                prev_state = cur;
                delta_acc_ms = 0;
                first_sample = false;
            }

            if last_emit.elapsed() >= Duration::from_millis(250) {
                last_emit = Instant::now();
                let st = GhostSessionStatus {
                    game_id: gid.clone(),
                    mode: "recording".to_string(),
                    active_ghost_id: Some(gid_id.clone()),
                    paused_focus_lost: false,
                    loop_mode: false,
                    elapsed_ms: total_elapsed_ms,
                    total_ms: total_elapsed_ms,
                    loop_count: 0,
                    frames_processed: frames.len(),
                    target_exe_name: gexe.clone(),
                    time_machine_checkpoint_id: tm_id.clone(),
                    status_text: format!(
                        "Recording {} delta events · {:.1}s",
                        frames.len(),
                        total_elapsed_ms as f64 / 1000.0
                    ),
                };
                update_status(&gid, st.clone());
                let _ = app_clone.emit("ghost-session-status", &st);
            }
        }

        let meta = GhostHeaderMetadata {
            ghost_id: gid_id.clone(),
            game_id: gid.clone(),
            game_title: gtitle,
            target_exe_name: gexe,
            label: glabel,
            created_at_epoch_sec: now,
            time_machine_checkpoint_id: tm_id,
            duration_ms: total_elapsed_ms.max(16),
            frame_count: frames.len(),
        };
        let encoded = encode_dkghost_bytes(&meta, &frames)?;
        let mut f = fs::File::create(&out_path)
            .map_err(|e| format!("Failed creating .dkghost '{}': {}", out_path.display(), e))?;
        f.write_all(&encoded)
            .map_err(|e| format!("Failed writing .dkghost '{}': {}", out_path.display(), e))?;
        read_ghost_summary(&out_path)
    });

    if let Ok(mut recs) = active_recorders().lock() {
        recs.insert(
            game_id,
            ActiveRecorderHandle {
                stop,
                join: Some(handle),
            },
        );
    }
    Ok(initial_status)
}

#[tauri::command]
pub async fn stop_ghost_recording(
    app: AppHandle,
    game_id: String,
) -> Result<GhostRecordingSummary, String> {
    let mut handle = active_recorders()
        .lock()
        .ok()
        .and_then(|mut m| m.remove(&game_id))
        .ok_or_else(|| "No active ghost recording for this game.".to_string())?;
    handle.stop.store(true, Ordering::Relaxed);
    let join = handle
        .join
        .take()
        .ok_or_else(|| "Ghost recorder thread already joined.".to_string())?;
    let summary = tokio::task::spawn_blocking(move || join.join())
        .await
        .map_err(|e| format!("Ghost recorder join error: {e}"))?
        .map_err(|_| "Ghost recorder thread panicked.".to_string())??;

    let idle = GhostSessionStatus {
        game_id: game_id.clone(),
        mode: "idle".to_string(),
        status_text: format!(
            "Saved '{}' ({} · ~{:.1} KB/min).",
            summary.label, summary.file_size_formatted, summary.kb_per_minute
        ),
        ..Default::default()
    };
    update_status(&game_id, idle.clone());
    let _ = app.emit("ghost-session-status", &idle);
    Ok(summary)
}

fn find_ghost_path(game_id: &str, ghost_id: &str) -> Result<PathBuf, String> {
    let dir = ghost_game_dir(game_id)?;
    let direct = dir.join(format!("{}.dkghost", ghost_id));
    if direct.is_file() {
        return Ok(direct);
    }
    if let Ok(rd) = fs::read_dir(&dir) {
        for ent in rd.flatten() {
            let p = ent.path();
            if p.extension().and_then(|s| s.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("dkghost")) {
                if let Ok(sum) = read_ghost_summary(&p) {
                    if sum.ghost_id == ghost_id {
                        return Ok(p);
                    }
                }
            }
        }
    }
    Err(format!("Ghost '{}' not found.", ghost_id))
}

fn stop_ghost_playback_internal(game_id: &str) -> bool {
    if let Ok(mut plays) = active_playbacks().lock() {
        if let Some(mut handle) = plays.remove(game_id) {
            handle.stop.store(true, Ordering::Relaxed);
            if let Some(j) = handle.join.take() {
                let _ = j.join();
            }
            return true;
        }
    }
    false
}

#[tauri::command]
pub async fn start_ghost_playback(
    app: AppHandle,
    game_id: String,
    ghost_id: String,
    loop_mode: Option<bool>,
    restore_time_machine: Option<bool>,
) -> Result<GhostSessionStatus, String> {
    let _ = stop_ghost_playback_internal(&game_id);
    let path = find_ghost_path(&game_id, &ghost_id)?;
    let bytes = fs::read(&path)
        .map_err(|e| format!("Failed loading .dkghost '{}': {}", path.display(), e))?;
    let (meta, frames) = decode_dkghost_bytes(&bytes)?;
    if frames.is_empty() {
        return Err("Ghost recording contains zero frames.".to_string());
    }

    if restore_time_machine.unwrap_or(true) {
        if let Some(ref cp_id) = meta.time_machine_checkpoint_id {
            let _ = crate::time_machine::restore_time_machine_checkpoint(
                app.clone(),
                game_id.clone(),
                cp_id.clone(),
            )
            .await;
        }
    }

    let loop_enabled = loop_mode.unwrap_or(false);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = Arc::clone(&stop);
    let app_clone = app.clone();
    let gid = game_id.clone();
    let target_exe = meta.target_exe_name.clone();
    let total_ms = meta.duration_ms.max(1);
    let total_frames = frames.len();
    let tm_cp = meta.time_machine_checkpoint_id.clone();

    let initial_status = GhostSessionStatus {
        game_id: game_id.clone(),
        mode: "playing".to_string(),
        active_ghost_id: Some(ghost_id.clone()),
        paused_focus_lost: false,
        loop_mode: loop_enabled,
        elapsed_ms: 0,
        total_ms,
        loop_count: 0,
        frames_processed: 0,
        target_exe_name: target_exe.clone(),
        time_machine_checkpoint_id: tm_cp.clone(),
        status_text: format!("Playing '{}'...", meta.label),
    };
    update_status(&game_id, initial_status.clone());
    let _ = app.emit("ghost-session-status", &initial_status);

    let join = std::thread::spawn(move || {
        let mut focus = GhostFocusMachine::new();
        let mut cursor = 0usize;
        let mut frame_acc_ms = 0u64;
        let mut last_ui_emit = Instant::now();
        let mut was_paused = false;

        while !stop_flag.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(SAMPLE_INTERVAL_MS));
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }
            let focused = is_target_game_focused(&target_exe);
            let (advanced, wrapped) =
                focus.advance(focused, SAMPLE_INTERVAL_MS, total_ms, loop_enabled);

            if focus.paused_focus_lost {
                if !was_paused {
                    was_paused = true;
                    // Release held virtual controller inputs cleanly when game loses focus
                    crate::publish_pipe_event(duke_core::pipe_ipc::DukeEvent::GhostInputFrame {
                        target_exe: target_exe.clone(),
                        pid: 0,
                        buttons: 0,
                        lt: 0,
                        rt: 0,
                        lx: 0,
                        ly: 0,
                        rx: 0,
                        ry: 0,
                        active: false,
                    });
                    let st = GhostSessionStatus {
                        game_id: gid.clone(),
                        mode: "playing".to_string(),
                        active_ghost_id: Some(meta.ghost_id.clone()),
                        paused_focus_lost: true,
                        loop_mode: loop_enabled,
                        elapsed_ms: focus.elapsed_ms,
                        total_ms,
                        loop_count: focus.loop_count,
                        frames_processed: cursor,
                        target_exe_name: target_exe.clone(),
                        time_machine_checkpoint_id: tm_cp.clone(),
                        status_text: format!("Auto-Paused (focus left {})", target_exe),
                    };
                    update_status(&gid, st.clone());
                    let _ = app_clone.emit("ghost-session-status", &st);
                }
                continue;
            }
            was_paused = false;

            if wrapped {
                cursor = 0;
                frame_acc_ms = 0;
            }

            if !advanced && !loop_enabled {
                break;
            }

            while cursor < total_frames {
                let next_delta = frames[cursor].delta_ms as u64;
                if frame_acc_ms.saturating_add(next_delta) <= focus.elapsed_ms {
                    frame_acc_ms = frame_acc_ms.saturating_add(next_delta);
                    let f = frames[cursor].state;
                    crate::publish_pipe_event(duke_core::pipe_ipc::DukeEvent::GhostInputFrame {
                        target_exe: target_exe.clone(),
                        pid: 0,
                        buttons: f.buttons,
                        lt: f.lt,
                        rt: f.rt,
                        lx: f.lx,
                        ly: f.ly,
                        rx: f.rx,
                        ry: f.ry,
                        active: true,
                    });
                    cursor += 1;
                } else {
                    break;
                }
            }

            if last_ui_emit.elapsed() >= Duration::from_millis(200) {
                last_ui_emit = Instant::now();
                let st = GhostSessionStatus {
                    game_id: gid.clone(),
                    mode: "playing".to_string(),
                    active_ghost_id: Some(meta.ghost_id.clone()),
                    paused_focus_lost: false,
                    loop_mode: loop_enabled,
                    elapsed_ms: focus.elapsed_ms,
                    total_ms,
                    loop_count: focus.loop_count,
                    frames_processed: cursor,
                    target_exe_name: target_exe.clone(),
                    time_machine_checkpoint_id: tm_cp.clone(),
                    status_text: format!(
                        "Playing {}/{} · Loop #{}",
                        cursor,
                        total_frames,
                        focus.loop_count + 1
                    ),
                };
                update_status(&gid, st.clone());
                let _ = app_clone.emit("ghost-session-status", &st);
            }
        }

        // Neutral release on end/stop
        crate::publish_pipe_event(duke_core::pipe_ipc::DukeEvent::GhostInputFrame {
            target_exe: target_exe.clone(),
            pid: 0,
            buttons: 0,
            lt: 0,
            rt: 0,
            lx: 0,
            ly: 0,
            rx: 0,
            ry: 0,
            active: false,
        });
        let done = GhostSessionStatus {
            game_id: gid.clone(),
            mode: "idle".to_string(),
            status_text: "Ghost playback finished.".to_string(),
            ..Default::default()
        };
        update_status(&gid, done.clone());
        let _ = app_clone.emit("ghost-session-status", &done);
    });

    if let Ok(mut plays) = active_playbacks().lock() {
        plays.insert(
            game_id,
            ActivePlaybackHandle {
                stop,
                join: Some(join),
            },
        );
    }
    Ok(initial_status)
}

#[tauri::command]
pub async fn stop_ghost_playback(app: AppHandle, game_id: String) -> Result<GhostSessionStatus, String> {
    let _ = stop_ghost_playback_internal(&game_id);
    let st = GhostSessionStatus {
        game_id: game_id.clone(),
        mode: "idle".to_string(),
        status_text: "Ghost playback stopped.".to_string(),
        ..Default::default()
    };
    update_status(&game_id, st.clone());
    let _ = app.emit("ghost-session-status", &st);
    Ok(st)
}

#[tauri::command]
pub async fn import_ghost_file(
    game_id: String,
    src_path: String,
) -> Result<GhostRecordingSummary, String> {
    tokio::task::spawn_blocking(move || {
        let src = PathBuf::from(src_path.trim());
        let bytes = fs::read(&src)
            .map_err(|e| format!("Failed reading shareable ghost '{}': {}", src.display(), e))?;
        let (mut meta, frames) = decode_dkghost_bytes(&bytes)?;
        let (_, exe_name) = resolve_game_meta(&game_id)?;
        meta.game_id = game_id.clone();
        if !exe_name.is_empty() {
            meta.target_exe_name = exe_name;
        }
        let dir = ghost_game_dir(&game_id)?;
        let dest = dir.join(format!("{}.dkghost", meta.ghost_id));
        let reencoded = encode_dkghost_bytes(&meta, &frames)?;
        let mut f = fs::File::create(&dest)
            .map_err(|e| format!("Failed writing imported ghost '{}': {}", dest.display(), e))?;
        f.write_all(&reencoded)
            .map_err(|e| format!("Failed writing imported ghost '{}': {}", dest.display(), e))?;
        read_ghost_summary(&dest)
    })
    .await
    .map_err(|e| format!("Ghost import failed: {e}"))?
}

#[tauri::command]
pub async fn export_ghost_file(
    game_id: String,
    ghost_id: String,
    dest_path: String,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let src = find_ghost_path(&game_id, &ghost_id)?;
        let dest = PathBuf::from(dest_path.trim());
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed creating export directory: {e}"))?;
        }
        fs::copy(&src, &dest)
            .map_err(|e| format!("Failed exporting .dkghost '{}': {}", dest.display(), e))?;
        Ok(format!("Exported shareable ghost to {}", dest.display()))
    })
    .await
    .map_err(|e| format!("Ghost export failed: {e}"))?
}

#[tauri::command]
pub async fn delete_ghost_recording(game_id: String, ghost_id: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let path = find_ghost_path(&game_id, &ghost_id)?;
        fs::remove_file(&path)
            .map_err(|e| format!("Failed deleting ghost '{}': {}", path.display(), e))?;
        let _ = atomic_write_string(&ghost_game_dir(&game_id)?.join(".last_gc"), "ok");
        Ok("Deleted ghost recording.".to_string())
    })
    .await
    .map_err(|e| format!("Ghost delete failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dkghost_roundtrip_stays_under_10kb_per_minute() {
        // Simulate 1 full minute (60,000 ms) of active speedrun inputs:
        // ~120 state transitions/minute (~2/sec rhythmic dodges, jumps, camera swings).
        let mut frames = Vec::with_capacity(240);
        for i in 0..240usize {
            let even = i % 2 == 0;
            frames.push(GhostDeltaFrame {
                delta_ms: 250,
                state: GhostControllerSample {
                    buttons: if even { 0x1000 } else { 0x0100 },
                    lt: if even { 0 } else { 200 },
                    rt: 180,
                    lx: ((i % 20) as i8 - 10) * 8,
                    ly: 64,
                    rx: 12,
                    ry: -8,
                },
            });
        }
        let meta = GhostHeaderMetadata {
            ghost_id: "test_1min".to_string(),
            game_id: "g1".to_string(),
            game_title: "Speedrun Game".to_string(),
            target_exe_name: "game.exe".to_string(),
            label: "Any% Practice".to_string(),
            created_at_epoch_sec: 1_730_000_000,
            time_machine_checkpoint_id: Some("cp_101".to_string()),
            duration_ms: 60_000,
            frame_count: frames.len(),
        };
        let encoded = encode_dkghost_bytes(&meta, &frames).expect("encode .dkghost");
        assert!(
            encoded.len() <= 10 * 1024,
            "1-minute ghost stream {} bytes exceeded 10 KB budget",
            encoded.len()
        );
        let (decoded_meta, decoded_frames) = decode_dkghost_bytes(&encoded).expect("decode .dkghost");
        assert_eq!(decoded_meta.time_machine_checkpoint_id.as_deref(), Some("cp_101"));
        assert_eq!(decoded_frames.len(), frames.len());
        assert_eq!(decoded_frames[7].state.buttons, frames[7].state.buttons);
    }

    #[test]
    fn ghost_focus_machine_pauses_on_focus_loss_and_loops_cleanly() {
        let mut fm = GhostFocusMachine::new();
        assert_eq!(fm.advance(true, 400, 1000, true), (true, false));
        assert_eq!(fm.elapsed_ms, 400);
        assert!(!fm.paused_focus_lost);

        // Alt-Tab away: elapsed_ms must freeze immediately
        assert_eq!(fm.advance(false, 500, 1000, true), (false, false));
        assert_eq!(fm.elapsed_ms, 400);
        assert!(fm.paused_focus_lost);

        // Return focus & wrap around 1000ms loop
        assert_eq!(fm.advance(true, 700, 1000, true), (true, true));
        assert!(!fm.paused_focus_lost);
        assert_eq!(fm.loop_count, 1);
        assert_eq!(fm.elapsed_ms, 100);
    }
}
