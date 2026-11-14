use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

pub const MAX_QUICK_RESUME_SLOTS: usize = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickResumeSlot {
    pub slot_index: usize,
    pub game_id: String,
    pub title: String,
    pub exe_path: String,
    pub install_dir: String,
    pub pid: u32,
    pub is_suspended: bool,
    pub working_set_mb: f64,
    pub standby_warmed_mb: f64,
    pub used_ioring_api: bool,
    pub platform_id: u32,
    pub last_switched_ts: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickResumeStateSummary {
    pub max_suspended_slots: usize,
    pub suspended_count: usize,
    pub active_count: usize,
    pub total_slot_ram_mb: f64,
    pub slots: Vec<QuickResumeSlot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickSwitchReport {
    pub switched_to_game_id: String,
    pub switched_to_title: String,
    pub suspended_game_id: Option<String>,
    pub suspended_title: Option<String>,
    pub freed_ram_mb: f64,
    pub standby_warmed_mb: f64,
    pub used_ioring_api: bool,
    pub frame_pacing_hz_label: String,
    pub slots: Vec<QuickResumeSlot>,
    pub summary: String,
}

static QUICK_RESUME_SLOTS: OnceLock<Mutex<Vec<QuickResumeSlot>>> = OnceLock::new();

fn slots_store() -> &'static Mutex<Vec<QuickResumeSlot>> {
    QUICK_RESUME_SLOTS.get_or_init(|| Mutex::new(Vec::new()))
}

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn renumber_slots(slots: &mut [QuickResumeSlot]) {
    slots.sort_by(|a, b| {
        a.is_suspended
            .cmp(&b.is_suspended)
            .then_with(|| b.last_switched_ts.cmp(&a.last_switched_ts))
    });
    for (idx, slot) in slots.iter_mut().enumerate() {
        slot.slot_index = idx + 1;
    }
}

/// Counts suspended slots cleanly and ensures at most `max_suspended` remain suspended
/// (returns any evicted suspended `QuickResumeSlot` entries, oldest first).
pub fn enforce_suspended_limit(
    slots: &mut Vec<QuickResumeSlot>,
    max_suspended: usize,
) -> Vec<QuickResumeSlot> {
    let mut suspended_indices: Vec<(usize, u64)> = slots
        .iter()
        .enumerate()
        .filter(|(_, s)| s.is_suspended)
        .map(|(i, s)| (i, s.last_switched_ts))
        .collect();
    if suspended_indices.len() <= max_suspended {
        renumber_slots(slots);
        return Vec::new();
    }
    suspended_indices.sort_by_key(|&(_, ts)| ts);
    let to_evict_count = suspended_indices.len() - max_suspended;
    let mut evict_ids: Vec<String> = Vec::with_capacity(to_evict_count);
    for (idx, _) in suspended_indices.into_iter().take(to_evict_count) {
        evict_ids.push(slots[idx].game_id.clone());
    }
    let mut evicted = Vec::new();
    slots.retain(|s| {
        if evict_ids.contains(&s.game_id) {
            evicted.push(s.clone());
            false
        } else {
            true
        }
    });
    renumber_slots(slots);
    evicted
}

pub fn register_launched_slot(
    game_id: &str,
    title: &str,
    exe_path: &str,
    install_dir: &str,
    pid: u32,
    platform_id: u32,
) {
    let Ok(mut guard) = slots_store().lock() else {
        return;
    };
    let ws = if pid > 0 {
        duke_core::win32_ffi::get_external_working_set_mb(pid)
    } else {
        0.0
    };
    let ts = now_ts();
    if let Some(existing) = guard.iter_mut().find(|s| s.game_id == game_id) {
        existing.title = title.to_string();
        existing.exe_path = exe_path.to_string();
        existing.install_dir = install_dir.to_string();
        existing.pid = pid;
        existing.is_suspended = false;
        existing.working_set_mb = ws;
        existing.platform_id = platform_id;
        existing.last_switched_ts = ts;
    } else {
        let next_idx = guard.len() + 1;
        guard.push(QuickResumeSlot {
            slot_index: next_idx,
            game_id: game_id.to_string(),
            title: title.to_string(),
            exe_path: exe_path.to_string(),
            install_dir: install_dir.to_string(),
            pid,
            is_suspended: false,
            working_set_mb: ws,
            standby_warmed_mb: 0.0,
            used_ioring_api: false,
            platform_id,
            last_switched_ts: ts,
        });
    }
    renumber_slots(&mut guard);
}

pub fn sync_slot_suspend_state(game_id: &str, is_suspended: bool, standby_warmed_mb: Option<f64>) {
    let Ok(mut guard) = slots_store().lock() else {
        return;
    };
    if let Some(slot) = guard.iter_mut().find(|s| s.game_id == game_id) {
        slot.is_suspended = is_suspended;
        slot.last_switched_ts = now_ts();
        if let Some(mb) = standby_warmed_mb {
            slot.standby_warmed_mb = mb;
        }
        if slot.pid > 0 {
            slot.working_set_mb = duke_core::win32_ffi::get_external_working_set_mb(slot.pid);
        }
    }
    let _ = enforce_suspended_limit(&mut guard, MAX_QUICK_RESUME_SLOTS);
}

pub fn unregister_slot(game_id: &str) {
    let Ok(mut guard) = slots_store().lock() else {
        return;
    };
    guard.retain(|s| s.game_id != game_id);
    renumber_slots(&mut guard);
}

pub fn build_summary_snapshot() -> QuickResumeStateSummary {
    let mut slots = slots_store()
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default();
    for slot in &mut slots {
        if slot.pid > 0 {
            let pids = crate::get_active_jobs_map()
                .lock()
                .ok()
                .and_then(|map| map.get(&slot.game_id).map(|job| job.active_pids()))
                .unwrap_or_else(|| vec![slot.pid]);
            let mut sum_mb = 0.0;
            if pids.is_empty() {
                sum_mb = duke_core::win32_ffi::get_external_working_set_mb(slot.pid);
            } else {
                for p in pids {
                    sum_mb += duke_core::win32_ffi::get_external_working_set_mb(p);
                }
            }
            slot.working_set_mb = (sum_mb * 100.0).round() / 100.0;
        }
    }
    renumber_slots(&mut slots);
    let suspended_count = slots.iter().filter(|s| s.is_suspended).count();
    let active_count = slots.len().saturating_sub(suspended_count);
    let total_slot_ram_mb = (slots.iter().map(|s| s.working_set_mb).sum::<f64>() * 100.0).round() / 100.0;

    QuickResumeStateSummary {
        max_suspended_slots: MAX_QUICK_RESUME_SLOTS,
        suspended_count,
        active_count,
        total_slot_ram_mb,
        slots,
    }
}

fn pids_for_session(game_id: &str, primary_pid: u32) -> Vec<u32> {
    let mut pids = crate::get_active_jobs_map()
        .lock()
        .ok()
        .and_then(|map| map.get(game_id).map(|job| job.active_pids()))
        .unwrap_or_default();
    if pids.is_empty() && primary_pid > 0 {
        pids.push(primary_pid);
    }
    pids
}

fn is_archive_or_rom_file(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "pak" | "rvz" | "chd" | "iso" | "wux" | "wua" | "cso" | "gcz" | "3ds" | "cci" | "nsp" | "xci"
    )
}

fn terminate_process_tree(pid: u32) {
    if pid == 0 {
        return;
    }
    #[cfg(target_os = "windows")]
    {
        let mut cmd = std::process::Command::new("taskkill");
        cmd.creation_flags(crate::CREATE_NO_WINDOW);
        let _ = cmd.args(["/PID", &pid.to_string(), "/T", "/F"]).output();
    }
}

#[tauri::command]
pub async fn get_quick_resume_slots() -> QuickResumeStateSummary {
    build_summary_snapshot()
}

#[tauri::command]
pub async fn quick_switch_game_slot(
    app: AppHandle,
    target_game_id: String,
) -> Result<QuickSwitchReport, String> {
    let target_clean = target_game_id.trim().to_string();
    if target_clean.is_empty() {
        return Err("Missing target Quick Resume game ID.".to_string());
    }

    let settings = crate::load_settings_standalone();
    let budget_pct = settings.nvme_cache_budget_pct.clamp(1, 40);

    let (active_others, target_opt) = {
        let guard = slots_store()
            .lock()
            .map_err(|_| "Quick Resume lock poisoned".to_string())?;
        let target = guard
            .iter()
            .find(|s| s.game_id == target_clean || s.title.eq_ignore_ascii_case(&target_clean))
            .cloned()
            .ok_or_else(|| format!("Game '{}' is not in an active Quick Resume slot.", target_clean))?;
        let others: Vec<QuickResumeSlot> = guard
            .iter()
            .filter(|s| s.game_id != target.game_id && !s.is_suspended)
            .cloned()
            .collect();
        (others, target)
    };

    let currently_suspended_other_count = slots_store()
        .lock()
        .map(|g| {
            g.iter()
                .filter(|s| s.is_suspended && s.game_id != target_opt.game_id)
                .count()
        })
        .unwrap_or(0);
    if !active_others.is_empty() && currently_suspended_other_count >= MAX_QUICK_RESUME_SLOTS {
        return Err(format!(
            "All {} Quick Resume suspended slots are occupied. Close or resume a suspended slot before freezing another game.",
            MAX_QUICK_RESUME_SLOTS
        ));
    }

    // 1. Freeze active Game A and trim private pages into Standby/Pagefile.
    let mut freed_ram_mb = 0.0;
    let mut suspended_gid: Option<String> = None;
    let mut suspended_title: Option<String> = None;
    for other in &active_others {
        let pids = pids_for_session(&other.game_id, other.pid);
        if !pids.is_empty() {
            freed_ram_mb += duke_core::win32_ffi::suspend_process_tree(&pids)?;
        }
        suspended_gid = Some(other.game_id.clone());
        suspended_title = Some(other.title.clone());
        if let Ok(mut sg) = crate::get_active_sessions().lock() {
            if let Some(sess) = sg.get_mut(&other.game_id) {
                sess.is_suspended = true;
            }
        }
        sync_slot_suspend_state(&other.game_id, true, None);
    }
    if !active_others.is_empty() {
        duke_core::win32_ffi::trim_current_working_set();
        let _ = duke_core::win32_ffi::end_frame_pacing_runtime();
    }

    // 2. Warm Target Game B's .pak / .rvz / .chd archives into Standby RAM using IoRing/PrefetchVirtualMemory.
    let warm_dir = Path::new(&target_opt.install_dir).to_path_buf();
    let warm_exe = Path::new(&target_opt.exe_path).to_path_buf();
    let (standby_warmed_mb, used_ioring_api) = tokio::task::spawn_blocking(move || {
        let mut total_mb = 0.0;
        let mut used_io = false;
        if warm_exe.is_file() && is_archive_or_rom_file(&warm_exe) {
            if let Ok(rep) = duke_core::nvme_prefetch::warm_single_file_with_budget_pct(
                &warm_exe,
                budget_pct,
            ) {
                total_mb += rep.warmed_mb;
                used_io |= rep.used_ioring_api;
            }
        }
        if warm_dir.is_dir() {
            if let Ok(rep) = duke_core::nvme_prefetch::warm_game_assets_with_budget_pct(
                &warm_dir,
                budget_pct,
            ) {
                total_mb += rep.warmed_mb;
                used_io |= rep.used_ioring_api;
            }
        }
        ((total_mb * 100.0).round() / 100.0, used_io)
    })
    .await
    .unwrap_or((0.0, false));

    // 3. Resume Target Game B and restore 0.5 ms frame pacing.
    let target_pids = pids_for_session(&target_opt.game_id, target_opt.pid);
    if !target_pids.is_empty() {
        duke_core::win32_ffi::resume_process_tree(&target_pids)?;
        duke_core::win32_ffi::boost_process_tree(&target_pids);
    }
    let pacing_status = if settings.frame_pacing_enabled {
        duke_core::win32_ffi::begin_frame_pacing_runtime()
    } else {
        duke_core::win32_ffi::inspect_frame_pacing_status()
    };

    if let Ok(mut sg) = crate::get_active_sessions().lock() {
        if let Some(sess) = sg.get_mut(&target_opt.game_id) {
            sess.is_suspended = false;
        }
    }
    if let Ok(mut guard) = slots_store().lock() {
        if let Some(s) = guard.iter_mut().find(|s| s.game_id == target_opt.game_id) {
            s.is_suspended = false;
            s.standby_warmed_mb = standby_warmed_mb;
            s.used_ioring_api = used_ioring_api;
            s.last_switched_ts = now_ts();
        }
        renumber_slots(&mut guard);
    }

    let summary_snap = build_summary_snapshot();
    let _ = app.emit("quick-resume-updated", &summary_snap);
    let _ = app.emit(
        "game-running-state",
        serde_json::json!({
            "running": true,
            "gameId": target_opt.game_id,
            "pid": target_opt.pid,
            "isSuspended": false,
            "gameBoost": true
        }),
    );

    let pace_label = if pacing_status.timer_resolution_engaged {
        format!("{:.2} ms", pacing_status.actual_100ns_units as f64 / 10_000.0)
    } else {
        "Default OS".to_string()
    };

    let summary_text = if let Some(ref prev) = suspended_title {
        format!(
            "Froze {} ({:.1} MB freed), warmed {:.1} MB into Standby RAM ({}), resumed {}.",
            prev,
            freed_ram_mb,
            standby_warmed_mb,
            if used_ioring_api { "IoRing" } else { "PrefetchVM" },
            target_opt.title
        )
    } else {
        format!(
            "Warmed {:.1} MB Standby RAM ({}) and resumed {}.",
            standby_warmed_mb,
            if used_ioring_api { "IoRing" } else { "PrefetchVM" },
            target_opt.title
        )
    };

    Ok(QuickSwitchReport {
        switched_to_game_id: target_opt.game_id,
        switched_to_title: target_opt.title,
        suspended_game_id: suspended_gid,
        suspended_title,
        freed_ram_mb: (freed_ram_mb * 100.0).round() / 100.0,
        standby_warmed_mb,
        used_ioring_api,
        frame_pacing_hz_label: pace_label,
        slots: summary_snap.slots,
        summary: summary_text,
    })
}

#[tauri::command]
pub async fn suspend_to_quick_resume_slot(
    app: AppHandle,
    game_id: String,
) -> Result<QuickSwitchReport, String> {
    let slot = slots_store()
        .lock()
        .map_err(|_| "Quick Resume lock poisoned".to_string())?
        .iter()
        .find(|s| s.game_id == game_id)
        .cloned()
        .ok_or_else(|| format!("No active Quick Resume slot found for '{}'.", game_id))?;

    let suspended_others = slots_store()
        .lock()
        .map(|g| g.iter().filter(|s| s.is_suspended && s.game_id != slot.game_id).count())
        .unwrap_or(0);
    if !slot.is_suspended && suspended_others >= MAX_QUICK_RESUME_SLOTS {
        return Err(format!(
            "Quick Resume already holds {} suspended games (maximum).",
            MAX_QUICK_RESUME_SLOTS
        ));
    }

    let pids = pids_for_session(&slot.game_id, slot.pid);
    let freed = if !pids.is_empty() {
        duke_core::win32_ffi::suspend_process_tree(&pids)?
    } else {
        0.0
    };
    duke_core::win32_ffi::trim_current_working_set();
    let _ = duke_core::win32_ffi::end_frame_pacing_runtime();
    if let Ok(mut sg) = crate::get_active_sessions().lock() {
        if let Some(sess) = sg.get_mut(&slot.game_id) {
            sess.is_suspended = true;
        }
    }
    sync_slot_suspend_state(&slot.game_id, true, None);
    let summary_snap = build_summary_snapshot();
    let _ = app.emit("quick-resume-updated", &summary_snap);

    Ok(QuickSwitchReport {
        switched_to_game_id: slot.game_id.clone(),
        switched_to_title: slot.title.clone(),
        suspended_game_id: Some(slot.game_id),
        suspended_title: Some(slot.title.clone()),
        freed_ram_mb: (freed * 100.0).round() / 100.0,
        standby_warmed_mb: slot.standby_warmed_mb,
        used_ioring_api: slot.used_ioring_api,
        frame_pacing_hz_label: "Released".to_string(),
        slots: summary_snap.slots,
        summary: format!(
            "Parked {} in Quick Resume Slot ({:.1} MB active RAM trimmed to Standby).",
            slot.title, freed
        ),
    })
}

#[tauri::command]
pub async fn remove_quick_resume_slot(
    app: AppHandle,
    game_id: String,
    terminate_process: bool,
) -> Result<QuickResumeStateSummary, String> {
    let existing = slots_store()
        .lock()
        .ok()
        .and_then(|g| g.iter().find(|s| s.game_id == game_id).cloned());
    if terminate_process {
        if let Some(slot) = existing {
            terminate_process_tree(slot.pid);
        }
    }
    unregister_slot(&game_id);
    if let Ok(mut sg) = crate::get_active_sessions().lock() {
        sg.remove(&game_id);
    }
    if let Ok(mut jg) = crate::get_active_jobs_map().lock() {
        jg.remove(&game_id);
    }
    let snap = build_summary_snapshot();
    let _ = app.emit("quick-resume-updated", &snap);
    Ok(snap)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_slot(id: &str, suspended: bool, ts: u64) -> QuickResumeSlot {
        QuickResumeSlot {
            slot_index: 0,
            game_id: id.to_string(),
            title: id.to_string(),
            exe_path: format!("{id}.exe"),
            install_dir: ".".to_string(),
            pid: 1000,
            is_suspended: suspended,
            working_set_mb: 512.0,
            standby_warmed_mb: 128.0,
            used_ioring_api: false,
            platform_id: 0,
            last_switched_ts: ts,
        }
    }

    #[test]
    fn quick_resume_enforces_three_suspended_slot_cap() {
        let mut slots = vec![
            mk_slot("game-active", false, 50),
            mk_slot("slot-oldest", true, 10),
            mk_slot("slot-mid-1", true, 20),
            mk_slot("slot-mid-2", true, 30),
            mk_slot("slot-newest", true, 40),
        ];

        let evicted = enforce_suspended_limit(&mut slots, MAX_QUICK_RESUME_SLOTS);
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].game_id, "slot-oldest");

        let suspended: Vec<&QuickResumeSlot> = slots.iter().filter(|s| s.is_suspended).collect();
        assert_eq!(suspended.len(), MAX_QUICK_RESUME_SLOTS);
        assert!(slots.iter().any(|s| s.game_id == "game-active" && s.slot_index == 1));
    }
}
