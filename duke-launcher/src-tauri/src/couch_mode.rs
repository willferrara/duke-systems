use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, RwLock};
use tauri::{AppHandle, Emitter};

static COUCH_BOOT_ARG: OnceLock<bool> = OnceLock::new();
static RADIAL_VISIBLE: AtomicBool = AtomicBool::new(false);
static VIRTUAL_KEYBOARD_VISIBLE: AtomicBool = AtomicBool::new(false);
static CONTROLLER_KIND: OnceLock<RwLock<String>> = OnceLock::new();
static RADIAL_SELECTED_SLOT: OnceLock<RwLock<Option<u8>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CouchRadialSlot {
    pub slot: u8,
    pub label: String,
    pub action_id: String,
    pub icon: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CouchModeStatus {
    pub active: bool,
    pub cli_booted: bool,
    pub controller_connected: bool,
    pub controller_kind: String,
    pub radial_visible: bool,
    pub radial_selected_slot: Option<u8>,
    pub virtual_keyboard_visible: bool,
    pub radial_slots: Vec<CouchRadialSlot>,
}

fn controller_kind_lock() -> &'static RwLock<String> {
    CONTROLLER_KIND.get_or_init(|| RwLock::new("xbox".to_string()))
}

fn radial_slot_lock() -> &'static RwLock<Option<u8>> {
    RADIAL_SELECTED_SLOT.get_or_init(|| RwLock::new(None))
}

pub fn init_cli_couch_flag(args: &[String]) -> bool {
    let is_couch = args.iter().any(|a| a.eq_ignore_ascii_case("--couch"));
    let _ = COUCH_BOOT_ARG.set(is_couch);
    is_couch
}

pub fn is_cli_couch_booted() -> bool {
    *COUCH_BOOT_ARG.get().unwrap_or(&false)
}

pub fn normalize_controller_kind(raw: &str) -> &'static str {
    let low = raw.trim().to_ascii_lowercase();
    if low.contains("dualsense") || low.contains("ps5") || low.contains("ps4") || low.contains("playstation") {
        "dualsense"
    } else if low.contains("switch") || low.contains("nintendo") {
        "switch"
    } else if low.contains("none") {
        "none"
    } else {
        "xbox"
    }
}

pub fn default_couch_radial_slots() -> Vec<CouchRadialSlot> {
    vec![
        CouchRadialSlot {
            slot: 0,
            label: "Couch Mode".to_string(),
            action_id: "toggle_couch_mode".to_string(),
            icon: "icon-gamepad-2".to_string(),
        },
        CouchRadialSlot {
            slot: 1,
            label: "Freeze / Resume".to_string(),
            action_id: "suspend_resume_game".to_string(),
            icon: "icon-pause".to_string(),
        },
        CouchRadialSlot {
            slot: 2,
            label: "Save Clip".to_string(),
            action_id: "save_replay_clip".to_string(),
            icon: "icon-video".to_string(),
        },
        CouchRadialSlot {
            slot: 3,
            label: "Purge RAM".to_string(),
            action_id: "purge_standby_ram".to_string(),
            icon: "icon-cpu".to_string(),
        },
        CouchRadialSlot {
            slot: 4,
            label: "Play Tonight".to_string(),
            action_id: "open_play_tonight".to_string(),
            icon: "icon-sparkles".to_string(),
        },
        CouchRadialSlot {
            slot: 5,
            label: "Command Bar".to_string(),
            action_id: "open_duke_pilot".to_string(),
            icon: "icon-terminal".to_string(),
        },
        CouchRadialSlot {
            slot: 6,
            label: "Quick Overlay".to_string(),
            action_id: "toggle_quick_overlay".to_string(),
            icon: "icon-zap".to_string(),
        },
        CouchRadialSlot {
            slot: 7,
            label: "Keyboard".to_string(),
            action_id: "toggle_virtual_keyboard".to_string(),
            icon: "icon-keyboard".to_string(),
        },
    ]
}

pub fn current_couch_status(settings_couch_mode: bool) -> CouchModeStatus {
    let pad = duke_core::win32_ffi::poll_xinput_state();
    let stored_kind = controller_kind_lock()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let kind = if stored_kind == "none" && pad.connected {
        "xbox".to_string()
    } else {
        stored_kind
    };
    CouchModeStatus {
        active: settings_couch_mode || is_cli_couch_booted(),
        cli_booted: is_cli_couch_booted(),
        controller_connected: pad.connected,
        controller_kind: kind,
        radial_visible: RADIAL_VISIBLE.load(Ordering::Relaxed),
        radial_selected_slot: *radial_slot_lock().read().unwrap_or_else(|e| e.into_inner()),
        virtual_keyboard_visible: VIRTUAL_KEYBOARD_VISIBLE.load(Ordering::Relaxed),
        radial_slots: default_couch_radial_slots(),
    }
}

pub fn handle_controller_guide_event(
    app: &AppHandle,
    action: &str,
    controller_kind: &str,
    radial_slot: Option<u8>,
) {
    let norm_kind = normalize_controller_kind(controller_kind);
    if norm_kind != "none" {
        *controller_kind_lock()
            .write()
            .unwrap_or_else(|e| e.into_inner()) = norm_kind.to_string();
    }

    match action {
        "toggle_couch_mode" => {
            let mut settings = crate::load_settings_from_disk(app);
            settings.couch_mode = !settings.couch_mode;
            let _ = crate::save_settings_to_disk(app, &settings);
            let status = current_couch_status(settings.couch_mode);
            let _ = app.emit("couch-mode-toggled", &status);
        }
        "radial_open" | "radial_hover" => {
            RADIAL_VISIBLE.store(true, Ordering::Release);
            *radial_slot_lock().write().unwrap_or_else(|e| e.into_inner()) = radial_slot;
            let settings = crate::load_settings_from_disk(app);
            let _ = app.emit("couch-radial-updated", &current_couch_status(settings.couch_mode));
        }
        "radial_close" => {
            RADIAL_VISIBLE.store(false, Ordering::Release);
            *radial_slot_lock().write().unwrap_or_else(|e| e.into_inner()) = None;
            let settings = crate::load_settings_from_disk(app);
            let _ = app.emit("couch-radial-updated", &current_couch_status(settings.couch_mode));
        }
        "toggle_virtual_keyboard" => {
            RADIAL_VISIBLE.store(false, Ordering::Release);
            let next = !VIRTUAL_KEYBOARD_VISIBLE.load(Ordering::Relaxed);
            VIRTUAL_KEYBOARD_VISIBLE.store(next, Ordering::Release);
            let settings = crate::load_settings_from_disk(app);
            let _ = app.emit("couch-radial-updated", &current_couch_status(settings.couch_mode));
        }
        "toggle_quick_overlay" => {
            RADIAL_VISIBLE.store(false, Ordering::Release);
            let app_c = app.clone();
            tauri::async_runtime::spawn(async move {
                let _ = crate::toggle_overlay_window(app_c, None).await;
            });
        }
        "suspend_resume_game" => {
            RADIAL_VISIBLE.store(false, Ordering::Release);
            let app_c = app.clone();
            tauri::async_runtime::spawn(async move {
                let _ = crate::suspend_or_resume_game(app_c, None, Some("toggle".to_string())).await;
            });
        }
        "purge_standby_ram" => {
            RADIAL_VISIBLE.store(false, Ordering::Release);
            duke_core::win32_ffi::trim_current_working_set();
        }
        other => {
            RADIAL_VISIBLE.store(false, Ordering::Release);
            let _ = app.emit("couch-radial-action", other);
        }
    }
}

#[tauri::command]
pub async fn get_couch_mode_status(app: AppHandle) -> Result<CouchModeStatus, String> {
    let settings = crate::load_settings_from_disk(&app);
    Ok(current_couch_status(settings.couch_mode))
}

#[tauri::command]
pub async fn set_couch_mode_state(
    app: AppHandle,
    enabled: bool,
    controller_kind: Option<String>,
) -> Result<CouchModeStatus, String> {
    if let Some(kind) = controller_kind {
        let norm = normalize_controller_kind(&kind);
        if norm != "none" {
            *controller_kind_lock()
                .write()
                .unwrap_or_else(|e| e.into_inner()) = norm.to_string();
        }
    }
    let mut settings = crate::load_settings_from_disk(&app);
    settings.couch_mode = enabled;
    crate::save_settings_to_disk(&app, &settings)?;
    let status = current_couch_status(enabled);
    let _ = app.emit("couch-mode-toggled", &status);
    Ok(status)
}

#[tauri::command]
pub async fn trigger_couch_radial_action(
    app: AppHandle,
    action_id: String,
    slot: Option<u8>,
) -> Result<CouchModeStatus, String> {
    let kind = controller_kind_lock()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    handle_controller_guide_event(&app, action_id.trim(), &kind, slot);
    let settings = crate::load_settings_from_disk(&app);
    Ok(current_couch_status(settings.couch_mode))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_xbox_and_dualsense_glyph_kinds() {
        assert_eq!(normalize_controller_kind("DualSense Wireless"), "dualsense");
        assert_eq!(normalize_controller_kind("PS5 Controller"), "dualsense");
        assert_eq!(normalize_controller_kind("Switch Pro"), "switch");
        assert_eq!(normalize_controller_kind("Xbox Series X"), "xbox");
    }

    #[test]
    fn exposes_eight_radial_hotbar_slots() {
        let slots = default_couch_radial_slots();
        assert_eq!(slots.len(), 8);
        assert_eq!(slots[0].action_id, "toggle_couch_mode");
        assert_eq!(slots[7].action_id, "toggle_virtual_keyboard");
    }
}
