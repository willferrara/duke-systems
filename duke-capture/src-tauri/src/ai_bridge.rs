use duke_core::shm_bus::DukeAudioTapShm;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

pub const DKF_MAGIC: &[u8; 8] = b"DUKEFRM1";

static AUDIO_TAP_SHM: OnceLock<Option<DukeAudioTapShm>> = OnceLock::new();
static LAST_MOTION_BITS: AtomicU32 = AtomicU32::new(0);
static LAST_SYS_RMS_BITS: AtomicU32 = AtomicU32::new(0);
static LAST_MIC_RMS_BITS: AtomicU32 = AtomicU32::new(0);
static LAST_TRANSIENT_SPIKE: AtomicBool = AtomicBool::new(false);
static LAST_SPIKE_EPOCH_MS: AtomicU64 = AtomicU64::new(0);
static PREV_FRAME_LUMA: OnceLock<Mutex<Vec<u8>>> = OnceLock::new();
static LATEST_BGRA_SNAPSHOT: OnceLock<Mutex<Option<LatestFrameBuffer>>> = OnceLock::new();

#[derive(Clone)]
struct LatestFrameBuffer {
    width: u32,
    height: u32,
    bgra: Vec<u8>,
    motion_score: f32,
    captured_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSignalTelemetry {
    pub system_peak_pct: u32,
    pub mic_peak_pct: u32,
    pub sys_rms: f32,
    pub mic_rms: f32,
    pub transient_spike: bool,
    pub motion_score: f32,
    pub ring_segments: usize,
    pub ring_duration_secs: f32,
    pub shm_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionFrameSnapshotInfo {
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub motion_score: f32,
    pub captured_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentSignalSummary {
    pub index: usize,
    pub duration_ms: u64,
    pub motion_score: f32,
    pub sys_rms: f32,
    pub transient_spike: bool,
    pub combined_score: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HighlightWindowSelection {
    pub start_idx: usize,
    pub end_idx_exclusive: usize,
    pub selected_duration_secs: f32,
    pub trimmed_dead_segments: usize,
    pub peak_intensity: u8,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn get_audio_tap_shm() -> Option<&'static DukeAudioTapShm> {
    AUDIO_TAP_SHM
        .get_or_init(|| DukeAudioTapShm::open_or_create().ok())
        .as_ref()
}

/// Pushes a 48 kHz interleaved stereo WASAPI packet into the lock-free 16 kHz AI shared-memory tap
/// and updates transient audio spike detection metrics.
pub fn on_wasapi_samples(is_mic: bool, stereo_48k: &[f32]) {
    if stereo_48k.is_empty() {
        return;
    }
    let rms = if let Some(shm) = get_audio_tap_shm() {
        shm.push_stereo_48k_decimated(is_mic, stereo_48k)
    } else {
        let sum_sq: f64 = stereo_48k.iter().map(|&s| (s as f64) * (s as f64)).sum();
        (sum_sq / stereo_48k.len().max(1) as f64).sqrt() as f32
    };

    if is_mic {
        LAST_MIC_RMS_BITS.store(rms.to_bits(), Ordering::Relaxed);
    } else {
        let prev = f32::from_bits(LAST_SYS_RMS_BITS.swap(rms.to_bits(), Ordering::Relaxed));
        if rms > 0.24 && rms > prev * 1.35 {
            LAST_TRANSIENT_SPIKE.store(true, Ordering::Release);
            LAST_SPIKE_EPOCH_MS.store(now_ms(), Ordering::Release);
        } else {
            let age_ms = now_ms().saturating_sub(LAST_SPIKE_EPOCH_MS.load(Ordering::Acquire));
            if age_ms > 2200 {
                LAST_TRANSIENT_SPIKE.store(false, Ordering::Release);
            }
        }
    }
}

/// Computes normalized inter-frame luminance change (`0.0..=1.0`) over a strided 16x16 grid
/// and caches the downsampled latest frame metadata for `Duke Vision`.
pub fn update_video_motion_and_frame(bgra: &[u8], width: u32, height: u32) -> f32 {
    if width < 16 || height < 16 || bgra.len() < (width as usize * height as usize * 4) {
        return 0.0;
    }

    let step_x = (width / 64).max(4) as usize;
    let step_y = (height / 36).max(4) as usize;
    let w = width as usize;
    let mut cur_luma = Vec::with_capacity((width as usize / step_x + 1) * (height as usize / step_y + 1));

    for y in (0..height as usize).step_by(step_y) {
        let row_off = y * w * 4;
        for x in (0..w).step_by(step_x) {
            let i = row_off + x * 4;
            if i + 2 < bgra.len() {
                // ITU-R BT.601 integer approximation on BGRA bytes
                let y_val = ((bgra[i + 2] as u32 * 77 + bgra[i + 1] as u32 * 150 + bgra[i] as u32 * 29)
                    >> 8) as u8;
                cur_luma.push(y_val);
            }
        }
    }

    let prev_lock = PREV_FRAME_LUMA.get_or_init(|| Mutex::new(Vec::new()));
    let motion = if let Ok(mut prev) = prev_lock.lock() {
        let score = if prev.len() == cur_luma.len() && !cur_luma.is_empty() {
            let sad: u64 = prev
                .iter()
                .zip(cur_luma.iter())
                .map(|(&a, &b)| a.abs_diff(b) as u64)
                .sum();
            ((sad as f64 / cur_luma.len() as f64) / 48.0).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        *prev = cur_luma;
        score
    } else {
        0.0
    };

    LAST_MOTION_BITS.store(motion.to_bits(), Ordering::Relaxed);
    if let Some(shm) = get_audio_tap_shm() {
        shm.set_motion_score(motion);
    }

    let latest_lock = LATEST_BGRA_SNAPSHOT.get_or_init(|| Mutex::new(None));
    if let Ok(mut slot) = latest_lock.try_lock() {
        *slot = Some(LatestFrameBuffer {
            width,
            height,
            bgra: bgra.to_vec(),
            motion_score: motion,
            captured_ms: now_ms(),
        });
    }

    motion
}

pub fn current_signal_snapshot() -> (f32, f32, f32, bool) {
    (
        f32::from_bits(LAST_MOTION_BITS.load(Ordering::Relaxed)),
        f32::from_bits(LAST_SYS_RMS_BITS.load(Ordering::Relaxed)),
        f32::from_bits(LAST_MIC_RMS_BITS.load(Ordering::Relaxed)),
        LAST_TRANSIENT_SPIKE.load(Ordering::Acquire),
    )
}

/// Computes an optimal highlight segment window between 15s and 30s around the highest multi-signal
/// action density, trimming silent/static leading & trailing menu or loading frames when requested.
pub fn select_highlight_window(
    segments: &[SegmentSignalSummary],
    trim_dead_frames: bool,
    target_duration_secs: f32,
) -> Option<HighlightWindowSelection> {
    if segments.is_empty() {
        return None;
    }

    let target_ms = (target_duration_secs.clamp(15.0, 30.0) * 1000.0).round() as u64;
    let total_ms: u64 = segments.iter().map(|s| s.duration_ms.max(500)).sum();
    if total_ms <= 15_500 || !trim_dead_frames {
        let mut start = segments.len();
        let mut accum = 0u64;
        while start > 0 && accum < target_ms {
            start -= 1;
            accum += segments[start].duration_ms.max(500);
        }
        let peak_f = segments[start..]
            .iter()
            .map(|s| s.combined_score)
            .fold(0.0f32, f32::max);
        return Some(HighlightWindowSelection {
            start_idx: start,
            end_idx_exclusive: segments.len(),
            selected_duration_secs: accum as f32 / 1000.0,
            trimmed_dead_segments: start,
            peak_intensity: (peak_f * 100.0).round().clamp(1.0, 100.0) as u8,
        });
    }

    let is_dead = |s: &SegmentSignalSummary| -> bool {
        !s.transient_spike && s.motion_score < 0.045 && s.sys_rms < 0.025 && s.combined_score < 0.12
    };

    let active_indices: Vec<usize> = segments
        .iter()
        .enumerate()
        .filter_map(|(idx, s)| if !is_dead(s) { Some(idx) } else { None })
        .collect();

    if active_indices.is_empty() {
        return select_highlight_window(segments, false, target_duration_secs);
    }

    // Find contiguous slice `[best_l, best_r)` whose total duration <= target_ms (or closest >= 15s)
    // and maximizes multi-signal action energy while shedding leading/trailing dead menu segments.
    let mut best_l = 0usize;
    let mut best_r = segments.len();
    let mut best_value = -1.0f32;

    for l in 0..segments.len() {
        let mut dur = 0u64;
        let mut score_sum = 0.0f32;
        let mut dead_in_span = 0usize;
        for (r_offset, seg) in segments[l..].iter().enumerate() {
            let seg_dur = seg.duration_ms.max(500);
            if dur + seg_dur > target_ms && dur >= 15_000 {
                break;
            }
            dur += seg_dur;
            score_sum += seg.combined_score + if seg.transient_spike { 0.35 } else { 0.0 };
            if is_dead(seg) {
                dead_in_span += 1;
            }
            let r = l + r_offset + 1;
            let trimmed_bonus = (segments.len().saturating_sub(r - l)) as f32 * 0.02;
            let penalty = dead_in_span as f32 * 0.08;
            let duration_fit = if (15_000..=30_000).contains(&dur) {
                0.25
            } else {
                0.0
            };
            let value = score_sum + trimmed_bonus + duration_fit - penalty;
            if (dur >= 15_000 || r == segments.len()) && value > best_value {
                best_value = value;
                best_l = l;
                best_r = r;
            }
        }
    }

    // Trim strictly dead edges while maintaining at least 15s if possible
    while best_l < best_r && is_dead(&segments[best_l]) {
        let candidate_dur: u64 = segments[best_l + 1..best_r]
            .iter()
            .map(|s| s.duration_ms.max(500))
            .sum();
        if candidate_dur >= 15_000 || active_indices.iter().any(|&idx| idx > best_l && idx < best_r) {
            best_l += 1;
        } else {
            break;
        }
    }
    while best_r > best_l + 1 && is_dead(&segments[best_r - 1]) {
        let candidate_dur: u64 = segments[best_l..best_r - 1]
            .iter()
            .map(|s| s.duration_ms.max(500))
            .sum();
        if candidate_dur >= 15_000 || active_indices.iter().any(|&idx| idx >= best_l && idx < best_r - 1) {
            best_r -= 1;
        } else {
            break;
        }
    }

    let final_dur_ms: u64 = segments[best_l..best_r].iter().map(|s| s.duration_ms).sum();
    let peak_f = segments[best_l..best_r]
        .iter()
        .map(|s| s.combined_score)
        .fold(0.0f32, f32::max);
    let trimmed = segments.len().saturating_sub(best_r - best_l);

    Some(HighlightWindowSelection {
        start_idx: best_l,
        end_idx_exclusive: best_r,
        selected_duration_secs: final_dur_ms as f32 / 1000.0,
        trimmed_dead_segments: trimmed,
        peak_intensity: (peak_f * 100.0).round().clamp(1.0, 100.0) as u8,
    })
}

/// Persists a compact `DUKEFRM1` raw BGRA snapshot (8-byte magic + width + height + BGRA pixels)
/// for low-latency cross-process OCR by `duke-launcher`'s `Duke Vision`.
pub fn write_vision_snapshot_file(
    path: &Path,
    width: u32,
    height: u32,
    bgra: &[u8],
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Snapshot dir error: {}", e))?;
    }
    let tmp = path.with_extension("dkf.tmp");
    let mut f = File::create(&tmp).map_err(|e| format!("Create snapshot temp error: {}", e))?;
    f.write_all(DKF_MAGIC)
        .and_then(|_| f.write_all(&width.to_le_bytes()))
        .and_then(|_| f.write_all(&height.to_le_bytes()))
        .and_then(|_| f.write_all(bgra))
        .and_then(|_| f.sync_data())
        .map_err(|e| format!("Write snapshot error: {}", e))?;
    drop(f);
    fs::rename(&tmp, path).map_err(|e| format!("Rename snapshot error: {}", e))?;
    Ok(())
}

pub fn get_cached_or_capture_frame<F>(app: &AppHandle, capture_fn: F) -> Result<VisionFrameSnapshotInfo, String>
where
    F: FnOnce() -> (u32, u32, Vec<u8>),
{
    let latest_lock = LATEST_BGRA_SNAPSHOT.get_or_init(|| Mutex::new(None));
    let recent = latest_lock.lock().ok().and_then(|guard| {
        guard.as_ref().and_then(|buf| {
            if now_ms().saturating_sub(buf.captured_ms) <= 750 && !buf.bgra.is_empty() {
                Some((buf.width, buf.height, buf.bgra.clone(), buf.motion_score, buf.captured_ms))
            } else {
                None
            }
        })
    });

    let (w, h, bgra, motion_score, captured_ms) = if let Some(hit) = recent {
        hit
    } else {
        let (cw, ch, cbgra) = capture_fn();
        let m = update_video_motion_and_frame(&cbgra, cw, ch);
        (cw, ch, cbgra, m, now_ms())
    };

    let data_dir = app
        .path()
        .app_local_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let snap_path = data_dir.join("ai_latest_frame.dkf");
    write_vision_snapshot_file(&snap_path, w, h, &bgra)?;

    Ok(VisionFrameSnapshotInfo {
        path: snap_path.to_string_lossy().to_string(),
        width: w,
        height: h,
        motion_score,
        captured_ms,
    })
}

pub fn build_ring_telemetry_queue<T>(
    sys_peak_pct: u32,
    mic_peak_pct: u32,
    ring: &VecDeque<T>,
    get_dur_ms: impl Fn(&T) -> u64,
) -> AiSignalTelemetry {
    let (motion, sys_rms, mic_rms, spike) = current_signal_snapshot();
    let total_ms: u64 = ring.iter().map(get_dur_ms).sum();
    AiSignalTelemetry {
        system_peak_pct: sys_peak_pct,
        mic_peak_pct,
        sys_rms,
        mic_rms,
        transient_spike: spike,
        motion_score: motion,
        ring_segments: ring.len(),
        ring_duration_secs: total_ms as f32 / 1000.0,
        shm_active: get_audio_tap_shm().is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_window_trims_dead_menu_frames_around_action_burst() {
        let segments = vec![
            SegmentSignalSummary { index: 0, duration_ms: 4000, motion_score: 0.01, sys_rms: 0.005, transient_spike: false, combined_score: 0.02 },
            SegmentSignalSummary { index: 1, duration_ms: 4000, motion_score: 0.02, sys_rms: 0.010, transient_spike: false, combined_score: 0.03 },
            SegmentSignalSummary { index: 2, duration_ms: 5000, motion_score: 0.65, sys_rms: 0.42, transient_spike: true, combined_score: 0.85 },
            SegmentSignalSummary { index: 3, duration_ms: 5000, motion_score: 0.72, sys_rms: 0.50, transient_spike: true, combined_score: 0.92 },
            SegmentSignalSummary { index: 4, duration_ms: 5000, motion_score: 0.58, sys_rms: 0.35, transient_spike: false, combined_score: 0.68 },
            SegmentSignalSummary { index: 5, duration_ms: 5000, motion_score: 0.48, sys_rms: 0.28, transient_spike: false, combined_score: 0.55 },
            SegmentSignalSummary { index: 6, duration_ms: 4000, motion_score: 0.01, sys_rms: 0.004, transient_spike: false, combined_score: 0.01 },
            SegmentSignalSummary { index: 7, duration_ms: 4000, motion_score: 0.01, sys_rms: 0.006, transient_spike: false, combined_score: 0.02 },
        ];

        let sel = select_highlight_window(&segments, true, 22.0).expect("window selection");
        assert!(sel.start_idx >= 2, "leading dead menu segments should be trimmed (got start={})", sel.start_idx);
        assert!(sel.end_idx_exclusive <= 6, "trailing dead menu segments should be trimmed (got end={})", sel.end_idx_exclusive);
        assert!(sel.selected_duration_secs >= 15.0 && sel.selected_duration_secs <= 30.0);
        assert!(sel.peak_intensity >= 85);
        assert!(sel.trimmed_dead_segments >= 3);
    }

    #[test]
    fn frame_motion_detects_scene_change() {
        let mut frame_a = vec![0u8; 64 * 64 * 4];
        let mut frame_b = vec![240u8; 64 * 64 * 4];
        for i in (3..frame_a.len()).step_by(4) {
            frame_a[i] = 255;
            frame_b[i] = 255;
        }
        let m1 = update_video_motion_and_frame(&frame_a, 64, 64);
        let m2 = update_video_motion_and_frame(&frame_b, 64, 64);
        assert!(m1 >= 0.0);
        assert!(m2 > 0.5, "bright white scene jump should register high motion: {}", m2);
    }
}
