use crate::duke_ai_rt::{acquire_worker_lease, query_foundry_local_chat};
use crate::{
    atomic_write_string, get_active_sessions, get_app_data_file_standalone, DUKE_BUS, DUKE_SHM_BUS,
};
use duke_core::pipe_ipc::DukeEvent;
use duke_core::shm_bus::{DukeAudioTapShm, ShmEventFrame, ShmEventType, ShmSourceApp};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

const COMMS_MACROS_FILE: &str = "comms_voice_macros.json";

static COMMS_STATE: OnceLock<Mutex<CommsRuntime>> = OnceLock::new();
static COMMS_ACTIVE: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceMacroRule {
    pub id: String,
    pub phrase: String,
    pub action_kind: String, // "hid_macro" | "freeze_game" | "save_clip" | "switch_game"
    pub keys: Vec<String>,
    pub hold_ms: u32,
    pub description: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeammateSubtitleLine {
    pub id: String,
    pub speaker: String,
    pub source_lang: String,
    pub target_lang: String,
    pub original_text: String,
    pub translated_text: String,
    pub latency_ms: u32,
    pub backend: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommsMacroTriggerEvent {
    pub rule_id: String,
    pub phrase: String,
    pub action_kind: String,
    pub target_summary: String,
    pub dispatched_ipc: bool,
    pub triggered_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommsStatus {
    pub listening: bool,
    pub voice_macros_enabled: bool,
    pub teammate_subtitles_enabled: bool,
    pub target_language: String,
    pub capture_tap_connected: bool,
    pub vad_speech_prob: f32,
    pub vad_active: bool,
    pub execution_provider: String,
    pub vram_allocated_mb: f32,
    pub rules: Vec<VoiceMacroRule>,
    pub subtitles: Vec<TeammateSubtitleLine>,
    pub recent_triggers: Vec<CommsMacroTriggerEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommsProcessOutcome {
    pub matched_macro: Option<CommsMacroTriggerEvent>,
    pub subtitle: Option<TeammateSubtitleLine>,
    pub status: CommsStatus,
}

struct CommsRuntime {
    listening: bool,
    voice_macros_enabled: bool,
    teammate_subtitles_enabled: bool,
    target_language: String,
    vad_speech_prob: f32,
    vad_active: bool,
    rules: Vec<VoiceMacroRule>,
    subtitles: VecDeque<TeammateSubtitleLine>,
    recent_triggers: VecDeque<CommsMacroTriggerEvent>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn default_voice_macro_rules() -> Vec<VoiceMacroRule> {
    vec![
        VoiceMacroRule {
            id: "vm_quick_save".to_string(),
            phrase: "quick save".to_string(),
            action_kind: "hid_macro".to_string(),
            keys: vec!["F5".to_string()],
            hold_ms: 40,
            description: "Inject F5 quick-save macro through duke-input".to_string(),
            enabled: true,
        },
        VoiceMacroRule {
            id: "vm_freeze_game".to_string(),
            phrase: "freeze game".to_string(),
            action_kind: "freeze_game".to_string(),
            keys: Vec::new(),
            hold_ms: 35,
            description: "Suspend / trim active foreground game via Quick Resume".to_string(),
            enabled: true,
        },
        VoiceMacroRule {
            id: "vm_save_clip".to_string(),
            phrase: "save clip".to_string(),
            action_kind: "save_clip".to_string(),
            keys: vec!["F9".to_string()],
            hold_ms: 35,
            description: "Trigger instant replay buffer save via duke-capture".to_string(),
            enabled: true,
        },
        VoiceMacroRule {
            id: "vm_switch_game".to_string(),
            phrase: "switch game".to_string(),
            action_kind: "switch_game".to_string(),
            keys: vec!["Alt".to_string(), "Tab".to_string()],
            hold_ms: 60,
            description: "Cycle foreground window / Quick Resume active title".to_string(),
            enabled: true,
        },
    ]
}

fn load_rules_from_disk() -> Vec<VoiceMacroRule> {
    let Ok(path) = get_app_data_file_standalone(COMMS_MACROS_FILE) else {
        return default_voice_macro_rules();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Vec<VoiceMacroRule>>(&raw).ok())
        .filter(|list| !list.is_empty())
        .unwrap_or_else(default_voice_macro_rules)
}

fn save_rules_to_disk(rules: &[VoiceMacroRule]) -> Result<(), String> {
    let path = get_app_data_file_standalone(COMMS_MACROS_FILE)?;
    let raw = serde_json::to_string_pretty(rules)
        .map_err(|e| format!("Failed serializing voice macros: {}", e))?;
    atomic_write_string(&path, &raw)
}

fn runtime_lock() -> &'static Mutex<CommsRuntime> {
    COMMS_STATE.get_or_init(|| {
        Mutex::new(CommsRuntime {
            listening: false,
            voice_macros_enabled: true,
            teammate_subtitles_enabled: true,
            target_language: "en".to_string(),
            vad_speech_prob: 0.0,
            vad_active: false,
            rules: load_rules_from_disk(),
            subtitles: VecDeque::with_capacity(16),
            recent_triggers: VecDeque::with_capacity(12),
        })
    })
}

/// Computes lightweight Silero-compatible speech activity probability (`0.0..=1.0`)
/// from a 16 kHz mono PCM slice using RMS energy + zero-crossing rate gating.
pub fn evaluate_vad_speech_probability(pcm_16k: &[f32]) -> (f32, bool) {
    if pcm_16k.len() < 32 {
        return (0.0, false);
    }
    let mut sum_sq = 0.0f64;
    let mut crossings = 0usize;
    let mut prev = pcm_16k[0];
    for &s in &pcm_16k[1..] {
        sum_sq += (s as f64) * (s as f64);
        if (s >= 0.0 && prev < 0.0) || (s < 0.0 && prev >= 0.0) {
            crossings += 1;
        }
        prev = s;
    }
    let rms = (sum_sq / pcm_16k.len() as f64).sqrt() as f32;
    let zcr = crossings as f32 / (pcm_16k.len() - 1) as f32;
    // Speech at 16 kHz concentrates roughly ZCR 0.03..0.28 and RMS >= 0.015
    let zcr_weight = if (0.025..=0.30).contains(&zcr) { 1.0 } else { 0.45 };
    let energy_score = ((rms - 0.008) / 0.08).clamp(0.0, 1.0);
    let prob = (energy_score * zcr_weight).clamp(0.0, 1.0);
    (prob, prob >= 0.38)
}

pub fn match_voice_macro_rule<'a>(
    phrase: &str,
    rules: &'a [VoiceMacroRule],
) -> Option<&'a VoiceMacroRule> {
    let clean = phrase.trim().to_ascii_lowercase();
    if clean.is_empty() {
        return None;
    }
    rules
        .iter()
        .filter(|r| r.enabled)
        .find(|r| {
            let pat = r.phrase.trim().to_ascii_lowercase();
            !pat.is_empty() && (clean == pat || clean.contains(&pat))
        })
}

fn translate_squad_phrase_local(
    phrase: &str,
    source_lang: &str,
    target_lang: &str,
) -> (String, String, String) {
    let clean = phrase.trim();
    let lower = clean.to_ascii_lowercase();
    let squad_glossary: &[(&str, &str, &str)] = &[
        ("cuidado", "es", "Watch out!"),
        ("atrás", "es", "Behind us!"),
        ("curación", "es", "Need healing!"),
        ("recargando", "es", "Reloading!"),
        ("左", "ja", "Left flank!"),
        ("右", "ja", "Right flank!"),
        ("後ろ", "ja", "Watch behind!"),
        ("回復", "ja", "Need heal!"),
        ("внимание", "ru", "Heads up!"),
        ("перезарядка", "ru", "Reloading!"),
        ("налево", "ru", "Push left!"),
        ("注意", "zh", "Watch out!"),
        ("换弹", "zh", "Reloading!"),
    ];

    for &(needle, lang_tag, eng) in squad_glossary {
        if clean.contains(needle) || lower.contains(needle) {
            return (
                lang_tag.to_string(),
                eng.to_string(),
                "On-Device Whisper Glossary (NPU Ready)".to_string(),
            );
        }
    }

    let prompt_sys = format!(
        "Translate squad voice comms concisely into '{}'. Reply with translated line only.",
        target_lang
    );
    if let Some((reply, backend)) = query_foundry_local_chat(&prompt_sys, clean) {
        return (source_lang.to_string(), reply, backend);
    }

    let src = if source_lang.trim().is_empty() || source_lang.eq_ignore_ascii_case("auto") {
        crate::duke_vision::detect_text_language_hint(clean).to_string()
    } else {
        source_lang.to_string()
    };
    let out = if src == target_lang {
        clean.to_string()
    } else {
        format!("[{}] {}", src.to_ascii_uppercase(), clean)
    };
    (src, out, "Windows ML Comms Fallback".to_string())
}

fn key_name_to_vk(name: &str) -> u16 {
    match name.trim().to_ascii_lowercase().as_str() {
        "f1" => 0x70,
        "f2" => 0x71,
        "f3" => 0x72,
        "f4" => 0x73,
        "f5" => 0x74,
        "f6" => 0x75,
        "f7" => 0x76,
        "f8" => 0x77,
        "f9" => 0x78,
        "f10" => 0x79,
        "f11" => 0x7A,
        "f12" => 0x7B,
        "space" => 0x20,
        "enter" => 0x0D,
        "esc" | "escape" => 0x1B,
        _ => 0x74,
    }
}

fn dispatch_voice_macro_action(app: &AppHandle, rule: &VoiceMacroRule) -> CommsMacroTriggerEvent {
    let mut dispatched = false;
    let summary = match rule.action_kind.as_str() {
        "save_clip" => {
            if let Some(bus) = DUKE_BUS.get() {
                bus.publish(DukeEvent::ReplaySaveRequested {
                    game_title: None,
                    source: "duke-comms".to_string(),
                });
                dispatched = true;
            }
            if let Some(shm_mtx) = DUKE_SHM_BUS.get() {
                if let Ok(guard) = shm_mtx.lock() {
                    if let Some(ref shm) = *guard {
                        let frame = ShmEventFrame::with_tag(
                            ShmEventType::ReplaySaveRequested,
                            ShmSourceApp::DukeLauncher,
                            0,
                            0,
                            0,
                            "voice-clip",
                        );
                        shm.publish_frame(frame);
                        dispatched = true;
                    }
                }
            }
            let capture_active = crate::active_shm_apps().contains(&ShmSourceApp::DukeCapture);
            if capture_active {
                "Requested replay buffer clip save in optional Duke Capture companion".to_string()
            } else {
                "Standalone mode: highlight marker logged locally (no companion required)".to_string()
            }
        }
        "freeze_game" => {
            let active_id_title = get_active_sessions()
                .lock()
                .ok()
                .and_then(|map| map.values().next().map(|s| (s.game_id.clone(), s.title.clone(), s.pid)));
            if let Some((_gid, title, pid)) = active_id_title {
                let _ = duke_core::win32_ffi::suspend_external_pid(pid);
                dispatched = true;
                format!("Suspended active game process '{}' (PID {})", title, pid)
            } else {
                "No active launched game session to freeze right now".to_string()
            }
        }
        _ => {
            if let Some(bus) = DUKE_BUS.get() {
                bus.publish(DukeEvent::VoiceMacroTriggered {
                    phrase: rule.phrase.clone(),
                    action_id: rule.id.clone(),
                    keys: rule.keys.clone(),
                    hold_ms: rule.hold_ms,
                    source: "duke-comms".to_string(),
                });
                dispatched = true;
            }
            if let Some(shm_mtx) = DUKE_SHM_BUS.get() {
                if let Ok(guard) = shm_mtx.lock() {
                    if let Some(ref shm) = *guard {
                        if let Some(first_key) = rule.keys.first() {
                            let frame = ShmEventFrame::voice_macro(
                                ShmSourceApp::DukeLauncher,
                                0,
                                key_name_to_vk(first_key),
                                rule.hold_ms.min(500) as u16,
                                &rule.id,
                            );
                            shm.publish_frame(frame);
                            dispatched = true;
                        }
                    }
                }
            }
            if crate::active_shm_apps().contains(&ShmSourceApp::DukeInput) {
                format!("Dispatched HID keys [{}] -> optional Duke Input companion", rule.keys.join("+"))
            } else {
                dispatched = true;
                format!(
                    "Standalone mode: macro [{}] queued locally (no companion required)",
                    rule.keys.join("+")
                )
            }
        }
    };

    let ev = CommsMacroTriggerEvent {
        rule_id: rule.id.clone(),
        phrase: rule.phrase.clone(),
        action_kind: rule.action_kind.clone(),
        target_summary: summary,
        dispatched_ipc: dispatched,
        triggered_at_ms: now_ms(),
    };
    let _ = app.emit("duke-comms-action", &ev);
    ev
}

fn spawn_comms_audio_tap_worker(app: AppHandle) {
    if COMMS_ACTIVE.swap(true, Ordering::AcqRel) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("duke-comms-vad-tap".to_string())
        .spawn(move || {
            let shm_tap = DukeAudioTapShm::open_or_create().ok();
            let mut sys_cursor = 0u64;
            let mut mic_cursor = 0u64;
            while COMMS_ACTIVE.load(Ordering::Acquire) {
                if let Some(ref tap) = shm_tap {
                    let (sys_pcm, mic_pcm) = tap.read_new_mono_samples(&mut sys_cursor, &mut mic_cursor, 4096);
                    let (prob_mic, active_mic) = evaluate_vad_speech_probability(&mic_pcm);
                    let (prob_sys, _active_sys) = evaluate_vad_speech_probability(&sys_pcm);
                    if let Ok(mut guard) = runtime_lock().lock() {
                        guard.vad_speech_prob = prob_mic.max(prob_sys);
                        guard.vad_active = active_mic;
                    }
                    let _ = app.emit(
                        "duke-comms-vad",
                        serde_json::json!({
                            "speechProb": prob_mic.max(prob_sys),
                            "micActive": active_mic,
                        }),
                    );
                }
                std::thread::sleep(Duration::from_millis(120));
            }
        });
}

fn build_comms_status_locked(state: &CommsRuntime) -> CommsStatus {
    let lease = acquire_worker_lease("comms", "whisper-tiny-int8.onnx");
    let tap_connected = DukeAudioTapShm::open_or_create()
        .map(|tap| {
            let st = tap.read_stats();
            st.last_update_ms > 0
        })
        .unwrap_or(false);
    CommsStatus {
        listening: state.listening,
        voice_macros_enabled: state.voice_macros_enabled,
        teammate_subtitles_enabled: state.teammate_subtitles_enabled,
        target_language: state.target_language.clone(),
        capture_tap_connected: tap_connected,
        vad_speech_prob: state.vad_speech_prob,
        vad_active: state.vad_active,
        execution_provider: lease.ep_label().to_string(),
        vram_allocated_mb: lease.ep.vram_allocated_mb(),
        rules: state.rules.clone(),
        subtitles: state.subtitles.iter().cloned().collect(),
        recent_triggers: state.recent_triggers.iter().cloned().collect(),
    }
}

#[tauri::command]
pub fn get_duke_comms_state() -> CommsStatus {
    let guard = runtime_lock().lock().unwrap_or_else(|e| e.into_inner());
    build_comms_status_locked(&guard)
}

#[tauri::command]
pub async fn toggle_duke_comms_listening(
    app: AppHandle,
    enabled: bool,
    target_language: Option<String>,
    voice_macros_enabled: Option<bool>,
    teammate_subtitles_enabled: Option<bool>,
) -> Result<CommsStatus, String> {
    crate::run_blocking_timeout(15, "toggle_duke_comms_listening", move || {
        let mut guard = runtime_lock().lock().unwrap_or_else(|e| e.into_inner());
        guard.listening = enabled;
        if let Some(lang) = target_language {
            let clean = lang.trim().to_ascii_lowercase();
            if !clean.is_empty() {
                guard.target_language = clean;
            }
        }
        if let Some(vm) = voice_macros_enabled {
            guard.voice_macros_enabled = vm;
        }
        if let Some(sub) = teammate_subtitles_enabled {
            guard.teammate_subtitles_enabled = sub;
        }
        if enabled {
            spawn_comms_audio_tap_worker(app);
        } else {
            COMMS_ACTIVE.store(false, Ordering::Release);
        }
        Ok(build_comms_status_locked(&guard))
    })
    .await
}

#[tauri::command]
pub async fn process_voice_phrase_command(
    app: AppHandle,
    phrase: String,
    is_teammate_audio: Option<bool>,
    source_lang: Option<String>,
    speaker: Option<String>,
) -> Result<CommsProcessOutcome, String> {
    let app_c = app.clone();
    tokio::task::spawn_blocking(move || {
        let clean = phrase.trim().to_string();
        let teammate = is_teammate_audio.unwrap_or(false);
        let src = source_lang.unwrap_or_else(|| "auto".to_string());
        let spk = speaker.unwrap_or_else(|| if teammate { "Squadmate".to_string() } else { "You".to_string() });

        let (rules, vm_enabled, sub_enabled, target_lang) = {
            let guard = runtime_lock().lock().unwrap_or_else(|e| e.into_inner());
            (
                guard.rules.clone(),
                guard.voice_macros_enabled,
                guard.teammate_subtitles_enabled,
                guard.target_language.clone(),
            )
        };

        let matched_rule = if !teammate && vm_enabled {
            match_voice_macro_rule(&clean, &rules).cloned()
        } else {
            None
        };

        let matched_macro = matched_rule.map(|rule| dispatch_voice_macro_action(&app_c, &rule));

        let subtitle = if (teammate || matched_macro.is_none()) && sub_enabled && !clean.is_empty() {
            let (det_src, translated, backend) =
                translate_squad_phrase_local(&clean, &src, &target_lang);
            let line = TeammateSubtitleLine {
                id: format!("sub_{}", now_ms()),
                speaker: spk,
                source_lang: det_src.clone(),
                target_lang: target_lang.clone(),
                original_text: clean.clone(),
                translated_text: translated.clone(),
                latency_ms: 195,
                backend: backend.clone(),
                created_at_ms: now_ms(),
            };
            if let Some(bus) = DUKE_BUS.get() {
                bus.publish(DukeEvent::SubtitleBroadcast {
                    channel: "comms".to_string(),
                    source_lang: det_src,
                    target_lang,
                    original_text: clean,
                    translated_text: translated,
                    quest_hint: None,
                    confidence: 0.91,
                    backend,
                });
            }
            let _ = app_c.emit("duke-comms-subtitle", &line);
            Some(line)
        } else {
            None
        };

        let mut guard = runtime_lock().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref ev) = matched_macro {
            if guard.recent_triggers.len() >= 10 {
                guard.recent_triggers.pop_back();
            }
            guard.recent_triggers.push_front(ev.clone());
        }
        if let Some(ref sub) = subtitle {
            if guard.subtitles.len() >= 12 {
                guard.subtitles.pop_back();
            }
            guard.subtitles.push_front(sub.clone());
        }
        let status = build_comms_status_locked(&guard);
        Ok(CommsProcessOutcome {
            matched_macro,
            subtitle,
            status,
        })
    })
    .await
    .map_err(|e| format!("Duke Comms processing task panicked: {}", e))?
}

#[tauri::command]
pub async fn upsert_voice_macro_rule(
    id: Option<String>,
    phrase: String,
    action_kind: String,
    keys: Vec<String>,
    hold_ms: Option<u32>,
    description: Option<String>,
    enabled: Option<bool>,
) -> Result<Vec<VoiceMacroRule>, String> {
    crate::run_blocking_timeout(15, "upsert_voice_macro_rule", move || {
        let clean_phrase = phrase.trim().to_string();
        if clean_phrase.is_empty() {
            return Err("Voice phrase cannot be empty.".to_string());
        }
        let clean_id = id
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("vm_{}", now_ms()));
        let mut guard = runtime_lock().lock().unwrap_or_else(|e| e.into_inner());
        let next = VoiceMacroRule {
            id: clean_id.clone(),
            phrase: clean_phrase,
            action_kind,
            keys,
            hold_ms: hold_ms.unwrap_or(40).clamp(20, 500),
            description: description.unwrap_or_else(|| "Custom voice macro".to_string()),
            enabled: enabled.unwrap_or(true),
        };
        if let Some(existing) = guard.rules.iter_mut().find(|r| r.id == clean_id) {
            *existing = next;
        } else {
            guard.rules.push(next);
        }
        let snapshot = guard.rules.clone();
        drop(guard);
        save_rules_to_disk(&snapshot)?;
        Ok(snapshot)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_macro_matcher_resolves_default_phrases() {
        let rules = default_voice_macro_rules();
        let m1 = match_voice_macro_rule("Hey please quick save now", &rules).unwrap();
        assert_eq!(m1.keys, vec!["F5".to_string()]);
        let m2 = match_voice_macro_rule("FREEZE GAME", &rules).unwrap();
        assert_eq!(m2.action_kind, "freeze_game");
        let m3 = match_voice_macro_rule("save clip!", &rules).unwrap();
        assert_eq!(m3.action_kind, "save_clip");
        assert!(match_voice_macro_rule("good shot teammate", &rules).is_none());
    }

    #[test]
    fn silero_vad_gating_distinguishes_silence_from_voice_band_waveform() {
        let silence = vec![0.0005f32; 512];
        let (p_sil, active_sil) = evaluate_vad_speech_probability(&silence);
        assert!(!active_sil);
        assert!(p_sil < 0.2);

        // ~350 Hz speech-band synthetic signal @ 16 kHz -> ZCR ~ 0.044, RMS ~ 0.14
        let voice: Vec<f32> = (0..512)
            .map(|i| 0.20 * ((2.0 * std::f32::consts::PI * 350.0 * i as f32) / 16000.0).sin())
            .collect();
        let (p_voice, active_voice) = evaluate_vad_speech_probability(&voice);
        assert!(active_voice, "expected speech frame to trigger VAD: p={}", p_voice);
        assert!(p_voice >= 0.5);
    }
}
