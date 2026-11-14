use crate::achievements::get_game_achievements;
use crate::duke_ai_rt::acquire_worker_lease;
use crate::{
    atomic_write_string, get_active_sessions, get_app_data_file_standalone,
    load_library_standalone, DUKE_BUS, DUKE_SHM_BUS,
};
use duke_core::pipe_ipc::DukeEvent;
use duke_core::shm_bus::{DukeAudioTapShm, ShmEventFrame, ShmSourceApp};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

const DIRECTOR_CLIPS_FILE: &str = "director_highlights.json";

static DIRECTOR_ENABLED: AtomicBool = AtomicBool::new(true);
static DIRECTOR_SENSITIVITY: AtomicU32 = AtomicU32::new(72); // threshold 0..100
static LAST_AUTO_CLIP_MS: AtomicU64 = AtomicU64::new(0);
static LATEST_AUDIO_PEAK_PCT: AtomicU32 = AtomicU32::new(0);
static LATEST_MOTION_BITS: AtomicU32 = AtomicU32::new(0);
static LATEST_TRANSIENT: AtomicBool = AtomicBool::new(false);
static RECENT_ACHIEVEMENT_HIT: OnceLock<Mutex<Option<(String, u64)>>> = OnceLock::new();
static HIGHLIGHT_LOG: OnceLock<Mutex<VecDeque<DirectorHighlightEntry>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectorSignalSample {
    pub offset_secs: f32,
    pub duration_secs: f32,
    pub motion_score: f32,
    pub audio_rms: f32,
    pub transient_spike: bool,
    pub achievement_hit: bool,
    pub etw_burst: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectorWindowPlan {
    pub start_secs: f32,
    pub end_secs: f32,
    pub clip_duration_secs: f32,
    pub dead_frames_trimmed_secs: f32,
    pub intensity_score: u8,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectorHighlightEntry {
    pub id: String,
    pub game_id: String,
    pub game_title: String,
    pub boss_or_achievement: String,
    pub tags: Vec<String>,
    pub intensity: u8,
    pub duration_secs: f32,
    pub dead_frames_trimmed_secs: f32,
    pub clip_path: Option<String>,
    pub auto_triggered: bool,
    pub execution_provider: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectorStateReport {
    pub auto_director_enabled: bool,
    pub sensitivity_threshold: u8,
    pub fused_intensity_score: u8,
    pub audio_peak_pct: u32,
    pub motion_score: f32,
    pub transient_spike: bool,
    pub recent_achievement: Option<String>,
    pub capture_tap_active: bool,
    pub execution_provider: String,
    pub vram_allocated_mb: f32,
    pub highlights: Vec<DirectorHighlightEntry>,
    pub recommended_window: Option<DirectorWindowPlan>,
}

static SIGNAL_HISTORY: OnceLock<Mutex<VecDeque<DirectorSignalSample>>> = OnceLock::new();

fn signal_history_lock() -> &'static Mutex<VecDeque<DirectorSignalSample>> {
    SIGNAL_HISTORY.get_or_init(|| Mutex::new(VecDeque::with_capacity(24)))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectorTriggerResult {
    pub entry: DirectorHighlightEntry,
    pub dispatched_to_capture: bool,
    pub message: String,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn load_highlights_from_disk() -> VecDeque<DirectorHighlightEntry> {
    let Ok(path) = get_app_data_file_standalone(DIRECTOR_CLIPS_FILE) else {
        return VecDeque::new();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Vec<DirectorHighlightEntry>>(&raw).ok())
        .map(VecDeque::from)
        .unwrap_or_default()
}

fn save_highlights_to_disk(items: &VecDeque<DirectorHighlightEntry>) -> Result<(), String> {
    let path = get_app_data_file_standalone(DIRECTOR_CLIPS_FILE)?;
    let vec_out: Vec<&DirectorHighlightEntry> = items.iter().collect();
    let raw = serde_json::to_string_pretty(&vec_out)
        .map_err(|e| format!("Failed serializing highlight catalog: {}", e))?;
    atomic_write_string(&path, &raw)
}

fn highlights_lock() -> &'static Mutex<VecDeque<DirectorHighlightEntry>> {
    HIGHLIGHT_LOG.get_or_init(|| Mutex::new(load_highlights_from_disk()))
}

fn recent_achievement_lock() -> &'static Mutex<Option<(String, u64)>> {
    RECENT_ACHIEVEMENT_HIT.get_or_init(|| Mutex::new(None))
}

/// Fuses gameplay telemetry signals into a `0..=100` action intensity score and auto-tags.
pub fn compute_fused_intensity(
    motion_score: f32,
    audio_peak_pct: u32,
    transient_spike: bool,
    recent_achievement: bool,
    etw_combat_burst: bool,
) -> (u8, Vec<String>) {
    let audio_norm = (audio_peak_pct as f32 / 100.0).clamp(0.0, 1.0);
    let mut score = motion_score.clamp(0.0, 1.0) * 38.0 + audio_norm * 32.0;
    let mut tags = Vec::new();

    if motion_score >= 0.45 {
        tags.push("High Action".to_string());
    }
    if transient_spike {
        score += 18.0;
        tags.push("Audio Transient".to_string());
    }
    if recent_achievement {
        score += 24.0;
        tags.push("Achievement / Boss".to_string());
    }
    if etw_combat_burst {
        score += 12.0;
        tags.push("Frame/Combat Burst".to_string());
    }

    let final_score = score.round().clamp(0.0, 100.0) as u8;
    if tags.is_empty() && final_score > 0 {
        tags.push("Gameplay".to_string());
    }
    (final_score, tags)
}

/// Evaluates a multi-signal timeline, trims quiet dead menu/loading frames at edges,
/// and chooses the tightest 15–30 second action window.
pub fn compute_highlight_trim_window(
    samples: &[DirectorSignalSample],
    desired_duration_secs: f32,
) -> Option<DirectorWindowPlan> {
    if samples.is_empty() {
        return None;
    }

    let total_dur: f32 = samples.iter().map(|s| s.duration_secs.max(0.5)).sum();
    let target = desired_duration_secs.clamp(15.0, 30.0);

    let sample_score = |s: &DirectorSignalSample| -> f32 {
        let mut sc = s.motion_score.clamp(0.0, 1.0) * 0.40 + (s.audio_rms * 1.3).clamp(0.0, 1.0) * 0.30;
        if s.transient_spike {
            sc += 0.22;
        }
        if s.achievement_hit {
            sc += 0.30;
        }
        if s.etw_burst {
            sc += 0.15;
        }
        sc
    };
    let is_dead = |s: &DirectorSignalSample| -> bool {
        !s.transient_spike
            && !s.achievement_hit
            && !s.etw_burst
            && s.motion_score < 0.05
            && s.audio_rms < 0.03
    };

    let mut best_l = 0usize;
    let mut best_r = samples.len();
    let mut best_obj = -1.0f32;

    for l in 0..samples.len() {
        let mut dur = 0.0f32;
        let mut score_sum = 0.0f32;
        let mut dead_dur = 0.0f32;
        for (off, smp) in samples[l..].iter().enumerate() {
            let sd = smp.duration_secs.max(0.5);
            if dur + sd > target && dur >= 15.0 {
                break;
            }
            dur += sd;
            score_sum += sample_score(smp);
            if is_dead(smp) {
                dead_dur += sd;
            }
            let r = l + off + 1;
            let in_range_bonus = if (15.0..=30.0).contains(&dur) { 0.30 } else { 0.0 };
            let obj = score_sum + in_range_bonus - dead_dur * 0.08;
            if (dur >= 15.0 || r == samples.len()) && obj > best_obj {
                best_obj = obj;
                best_l = l;
                best_r = r;
            }
        }
    }

    while best_l + 1 < best_r && is_dead(&samples[best_l]) {
        let rem: f32 = samples[best_l + 1..best_r].iter().map(|s| s.duration_secs.max(0.5)).sum();
        if rem >= 15.0 || !is_dead(&samples[best_l + 1]) {
            best_l += 1;
        } else {
            break;
        }
    }
    while best_r > best_l + 1 && is_dead(&samples[best_r - 1]) {
        let rem: f32 = samples[best_l..best_r - 1].iter().map(|s| s.duration_secs.max(0.5)).sum();
        if rem >= 15.0 || !is_dead(&samples[best_r - 2]) {
            best_r -= 1;
        } else {
            break;
        }
    }

    let selected = &samples[best_l..best_r];
    let start_secs = selected.first().map(|s| s.offset_secs).unwrap_or(0.0);
    let clip_dur: f32 = selected.iter().map(|s| s.duration_secs.max(0.5)).sum();
    let trimmed_secs = (total_dur - clip_dur).max(0.0);

    let peak_motion = selected.iter().map(|s| s.motion_score).fold(0.0f32, f32::max);
    let peak_rms = selected.iter().map(|s| s.audio_rms).fold(0.0f32, f32::max);
    let any_transient = selected.iter().any(|s| s.transient_spike);
    let any_ach = selected.iter().any(|s| s.achievement_hit);
    let any_etw = selected.iter().any(|s| s.etw_burst);

    let (intensity, mut tags) = compute_fused_intensity(
        peak_motion,
        (peak_rms * 140.0).round() as u32,
        any_transient,
        any_ach,
        any_etw,
    );
    if trimmed_secs > 1.0 {
        tags.push(format!("Trimmed {:.1}s Dead Menus", trimmed_secs));
    }

    Some(DirectorWindowPlan {
        start_secs,
        end_secs: start_secs + clip_dur,
        clip_duration_secs: clip_dur,
        dead_frames_trimmed_secs: trimmed_secs,
        intensity_score: intensity,
        tags,
    })
}

fn dispatch_highlight_to_capture(
    app: &AppHandle,
    game_id: &str,
    game_title: &str,
    boss_or_achievement: Option<String>,
    intensity: u8,
    duration_secs: f32,
    auto_triggered: bool,
) -> DirectorTriggerResult {
    let lease = acquire_worker_lease("director", "keyframe-scorer.onnx");
    let dur = duration_secs.clamp(15.0, 30.0);
    let mut dispatched = false;

    if let Some(bus) = DUKE_BUS.get() {
        bus.publish(DukeEvent::HighlightClipRequested {
            game_title: Some(game_title.to_string()),
            achievement_or_boss: boss_or_achievement.clone(),
            intensity,
            trim_start_offset_secs: 0.0,
            duration_secs: dur,
            trim_dead_frames: true,
            source: "duke-director".to_string(),
        });
        dispatched = true;
    }

    if let Some(shm_mtx) = DUKE_SHM_BUS.get() {
        if let Ok(guard) = shm_mtx.lock() {
            if let Some(ref shm) = *guard {
                let tag = boss_or_achievement.as_deref().unwrap_or(game_title);
                let frame = ShmEventFrame::highlight_peak(
                    ShmSourceApp::DukeLauncher,
                    0,
                    intensity,
                    dur.round() as u8,
                    tag,
                );
                shm.publish_frame(frame);
                dispatched = true;
            }
        }
    }

    let mut tags = vec!["Auto-Directed".to_string(), format!("{:.0}s Window", dur)];
    if let Some(ref lbl) = boss_or_achievement {
        if !lbl.trim().is_empty() {
            tags.push(lbl.trim().to_string());
        }
    }

    let entry = DirectorHighlightEntry {
        id: format!("dir_{}", now_ms()),
        game_id: game_id.to_string(),
        game_title: game_title.to_string(),
        boss_or_achievement: boss_or_achievement.unwrap_or_else(|| "Peak Moment".to_string()),
        tags,
        intensity,
        duration_secs: dur,
        dead_frames_trimmed_secs: 4.5,
        clip_path: None,
        auto_triggered,
        execution_provider: lease.ep_label().to_string(),
        created_at_ms: now_ms(),
    };

    if let Ok(mut guard) = highlights_lock().lock() {
        guard.push_front(entry.clone());
        while guard.len() > 50 {
            guard.pop_back();
        }
        let _ = save_highlights_to_disk(&guard);
    }

    let _ = app.emit("duke-director-highlight", &entry);
    DirectorTriggerResult {
        message: if dispatched {
            format!(
                "Dispatched {:.0}s highlight (intensity {}) to duke-capture.",
                dur, intensity
            )
        } else {
            format!(
                "Logged {:.0}s highlight marker locally (launch Duke Capture replay buffer to save MP4s).",
                dur
            )
        },
        entry,
        dispatched_to_capture: dispatched,
    }
}

/// Called from `main.rs` pipe/SHM listeners when `CaptureSignalTelemetry` arrives from `duke-capture`.
pub fn on_capture_telemetry_pulse(
    app: &AppHandle,
    system_peak_pct: u32,
    motion_score: f32,
    transient_spike: bool,
) {
    LATEST_AUDIO_PEAK_PCT.store(system_peak_pct, Ordering::Relaxed);
    LATEST_MOTION_BITS.store(motion_score.to_bits(), Ordering::Relaxed);
    LATEST_TRANSIENT.store(transient_spike, Ordering::Release);

    if !DIRECTOR_ENABLED.load(Ordering::Acquire) {
        return;
    }

    let recent_ach = recent_achievement_lock()
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .and_then(|(slug, ts)| (now_ms().saturating_sub(ts) <= 10_000).then_some(slug));

    let (intensity, _tags) = compute_fused_intensity(
        motion_score,
        system_peak_pct,
        transient_spike,
        recent_ach.is_some(),
        false,
    );
    if let Ok(mut hist) = signal_history_lock().lock() {
        let offset = hist.back().map(|s| s.offset_secs + s.duration_secs).unwrap_or(0.0);
        hist.push_back(DirectorSignalSample {
            offset_secs: offset,
            duration_secs: 2.0,
            motion_score,
            audio_rms: (system_peak_pct as f32 / 100.0).clamp(0.0, 1.0),
            transient_spike,
            achievement_hit: recent_ach.is_some(),
            etw_burst: false,
        });
        while hist.len() > 24 {
            hist.pop_front();
        }
    }
    let threshold = DIRECTOR_SENSITIVITY.load(Ordering::Relaxed) as u8;
    let now = now_ms();
    let prev = LAST_AUTO_CLIP_MS.load(Ordering::Acquire);
    if intensity >= threshold && now.saturating_sub(prev) >= 25_000 {
        LAST_AUTO_CLIP_MS.store(now, Ordering::Release);
        let active = get_active_sessions()
            .lock()
            .ok()
            .and_then(|m| m.values().next().map(|s| (s.game_id.clone(), s.title.clone())))
            .unwrap_or_else(|| ("active".to_string(), "Active Game".to_string()));
        let _ = dispatch_highlight_to_capture(app, &active.0, &active.1, recent_ach, intensity, 22.0, true);
    }
}

/// Called when an achievement unlocks cleanly to boost director event fusion.
pub fn on_achievement_unlocked_pulse(app: &AppHandle, game_title: &str, slug: &str) {
    if let Ok(mut guard) = recent_achievement_lock().lock() {
        *guard = Some((slug.to_string(), now_ms()));
    }
    if !DIRECTOR_ENABLED.load(Ordering::Acquire) {
        return;
    }
    let now = now_ms();
    let prev = LAST_AUTO_CLIP_MS.load(Ordering::Acquire);
    if now.saturating_sub(prev) >= 12_000 {
        LAST_AUTO_CLIP_MS.store(now, Ordering::Release);
        let _ = dispatch_highlight_to_capture(
            app,
            "",
            game_title,
            Some(slug.to_string()),
            88,
            24.0,
            true,
        );
    }
}

/// Associates saved clip file paths emitted back from `duke-capture`'s `DukeEvent::ClipSaved`.
pub fn on_clip_saved_pulse(game_title: &str, file_path: &str, duration_secs: f32) {
    if let Ok(mut guard) = highlights_lock().lock() {
        let now = now_ms();
        if let Some(item) = guard.iter_mut().find(|h| {
            h.clip_path.is_none()
                && now.saturating_sub(h.created_at_ms) <= 20_000
                && (h.game_title.eq_ignore_ascii_case(game_title)
                    || file_path
                        .to_ascii_lowercase()
                        .contains(&h.game_title.to_ascii_lowercase()))
        }) {
            item.clip_path = Some(file_path.to_string());
            item.duration_secs = duration_secs;
            let _ = save_highlights_to_disk(&guard);
        }
    }
}

#[tauri::command]
pub async fn get_duke_director_state(
    app: AppHandle,
    game_id: Option<String>,
) -> Result<DirectorStateReport, String> {
    let mut audio_peak_pct = LATEST_AUDIO_PEAK_PCT.load(Ordering::Relaxed);
    let mut motion_score = f32::from_bits(LATEST_MOTION_BITS.load(Ordering::Relaxed));
    let transient_spike = LATEST_TRANSIENT.load(Ordering::Acquire);
    let mut tap_active = false;

    if let Ok(shm) = DukeAudioTapShm::open_or_create() {
        let stats = shm.read_stats();
        if stats.last_update_ms > 0 {
            tap_active = true;
            audio_peak_pct = audio_peak_pct.max(((stats.sys_rms.sqrt()) * 100.0).round() as u32);
            motion_score = motion_score.max(stats.motion_score);
        }
    }

    let mut recent_achievement = recent_achievement_lock()
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|(s, _)| s.clone()));

    if recent_achievement.is_none() {
        if let Some(ref gid) = game_id.as_deref().filter(|s| !s.trim().is_empty()) {
            if let Ok(report) = get_game_achievements(app, gid.to_string()).await {
                if let Some(latest) = report.achievements.iter().find(|a| a.unlocked) {
                    recent_achievement = Some(latest.display_name.clone());
                }
            }
        }
    }

    let (fused_intensity_score, _) = compute_fused_intensity(
        motion_score,
        audio_peak_pct,
        transient_spike,
        recent_achievement.is_some(),
        false,
    );
    let lease = acquire_worker_lease("director", "keyframe-scorer.onnx");

    let all_highlights: Vec<DirectorHighlightEntry> = highlights_lock()
        .lock()
        .map(|g| {
            if let Some(gid) = game_id.as_deref().filter(|s| !s.trim().is_empty()) {
                let matching: Vec<_> = g
                    .iter()
                    .filter(|h| h.game_id == gid || h.game_id.is_empty())
                    .cloned()
                    .collect();
                if matching.is_empty() {
                    g.iter().take(12).cloned().collect()
                } else {
                    matching
                }
            } else {
                g.iter().take(16).cloned().collect()
            }
        })
        .unwrap_or_default();

    let recommended_window = signal_history_lock()
        .lock()
        .ok()
        .map(|h| h.iter().cloned().collect::<Vec<_>>())
        .and_then(|samples| compute_highlight_trim_window(&samples, 22.0));

    Ok(DirectorStateReport {
        auto_director_enabled: DIRECTOR_ENABLED.load(Ordering::Acquire),
        sensitivity_threshold: DIRECTOR_SENSITIVITY.load(Ordering::Relaxed) as u8,
        fused_intensity_score,
        audio_peak_pct,
        motion_score,
        transient_spike,
        recent_achievement,
        capture_tap_active: tap_active,
        execution_provider: lease.ep_label().to_string(),
        vram_allocated_mb: lease.ep.vram_allocated_mb(),
        highlights: all_highlights,
        recommended_window,
    })
}

#[tauri::command]
pub async fn toggle_duke_director(
    app: AppHandle,
    enabled: bool,
    sensitivity: Option<u8>,
) -> Result<DirectorStateReport, String> {
    DIRECTOR_ENABLED.store(enabled, Ordering::Release);
    if let Some(s) = sensitivity {
        DIRECTOR_SENSITIVITY.store(s.clamp(20, 98) as u32, Ordering::Relaxed);
    }
    get_duke_director_state(app, None).await
}

#[tauri::command]
pub async fn trigger_director_highlight_now(
    app: AppHandle,
    game_id: Option<String>,
    game_title: Option<String>,
    boss_or_achievement: Option<String>,
    target_duration_secs: Option<f32>,
) -> Result<DirectorTriggerResult, String> {
    let gid = game_id.unwrap_or_default();
    let title = if let Some(t) = game_title.filter(|s| !s.trim().is_empty()) {
        t
    } else if !gid.is_empty() {
        load_library_standalone()
            .ok()
            .and_then(|lib| lib.into_iter().find(|g| g.id == gid).map(|g| g.title))
            .unwrap_or_else(|| "Active Game".to_string())
    } else {
        "Active Game".to_string()
    };

    let dur = target_duration_secs.unwrap_or(22.0).clamp(15.0, 30.0);
    Ok(dispatch_highlight_to_capture(
        &app,
        &gid,
        &title,
        boss_or_achievement,
        90,
        dur,
        false,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_window_trims_dead_menus_and_enforces_15_to_30s_span() {
        let samples = vec![
            DirectorSignalSample { offset_secs: 0.0, duration_secs: 5.0, motion_score: 0.01, audio_rms: 0.01, transient_spike: false, achievement_hit: false, etw_burst: false },
            DirectorSignalSample { offset_secs: 5.0, duration_secs: 5.0, motion_score: 0.02, audio_rms: 0.01, transient_spike: false, achievement_hit: false, etw_burst: false },
            DirectorSignalSample { offset_secs: 10.0, duration_secs: 6.0, motion_score: 0.65, audio_rms: 0.48, transient_spike: true, achievement_hit: false, etw_burst: true },
            DirectorSignalSample { offset_secs: 16.0, duration_secs: 6.0, motion_score: 0.75, audio_rms: 0.55, transient_spike: true, achievement_hit: true, etw_burst: true },
            DirectorSignalSample { offset_secs: 22.0, duration_secs: 6.0, motion_score: 0.50, audio_rms: 0.35, transient_spike: false, achievement_hit: false, etw_burst: false },
            DirectorSignalSample { offset_secs: 28.0, duration_secs: 5.0, motion_score: 0.01, audio_rms: 0.01, transient_spike: false, achievement_hit: false, etw_burst: false },
        ];
        let plan = compute_highlight_trim_window(&samples, 20.0).expect("window");
        assert!(plan.start_secs >= 10.0, "expected leading menu trimmed: {:?}", plan);
        assert!(plan.end_secs <= 28.5, "expected trailing menu trimmed: {:?}", plan);
        assert!(plan.clip_duration_secs >= 15.0 && plan.clip_duration_secs <= 30.0);
        assert!(plan.dead_frames_trimmed_secs >= 10.0);
        assert!(plan.intensity_score >= 80);
    }
}
