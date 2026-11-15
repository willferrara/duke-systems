//! Native Windows 11 "Xbox Full Screen Experience (FSE)" Console Shell Mode (`fse_shell.rs`).
//!
//! Turns any Windows 11 PC or gaming handheld into a `<25 MB` console OS by:
//! - Providing a 1-click **"Set Duke Deck as Windows Gaming Home"** toggle that registers
//!   `Duke Launcher.exe --couch --fse` in `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
//!   and configures Windows 11 GameBar / FSE shell hooks (`HKCU\Software\Microsoft\GameBar`).
//! - Binding the Xbox / PlayStation Guide button (`XInput 0x0400`, `VK_GAMEPAD_VIEW`, and
//!   `Win+G` / `Alt+G` chord) to toggle the Duke Overlay HUD cleanly.
//! - Suspending curated non-essential Windows background bloat on boot (`NtSuspendProcess`) and
//!   restoring them cleanly (`NtResumeProcess`) when FSE mode is turned off.

use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

use crate::{atomic_write_string, get_app_data_file_standalone};

const FSE_STATE_FILE: &str = "fse_shell_state.json";
const REG_RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const REG_RUN_VALUE: &str = "DukeLauncherFSE";
const REG_GAMEBAR_KEY: &str = r"Software\Microsoft\GameBar";
const REG_DUKE_FSE_KEY: &str = r"Software\DukeSystems\DukeLauncher\FSE";

static FSE_CLI_ARG: OnceLock<bool> = OnceLock::new();
static FSE_GUIDE_ACTIVE: AtomicBool = AtomicBool::new(false);
static FSE_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// Curated list of non-essential user-space background bloat processes safe to pause during FSE Console Mode.
pub const NON_ESSENTIAL_BLOAT_EXES: &[&str] = &[
    "OneDrive.exe",
    "PhoneExperienceHost.exe",
    "YourPhone.exe",
    "Widgets.exe",
    "WidgetService.exe",
    "GameBarPresenceWriter.exe",
    "Copilot.exe",
    "Cortana.exe",
    "Teams.exe",
    "ms-teams.exe",
];

/// Critical OS and launcher processes that must NEVER be suspended under any circumstance.
const PROTECTED_SYSTEM_EXES: &[&str] = &[
    "system",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "svchost.exe",
    "dwm.exe",
    "explorer.exe",
    "sihost.exe",
    "ctfmon.exe",
    "audiodg.exe",
    "conhost.exe",
    "fontdrvhost.exe",
    "duke-launcher.exe",
    "msedgewebview2.exe",
];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SuspendedBloatProcess {
    pub pid: u32,
    pub exe_name: String,
    pub freed_mb: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct FseShellState {
    pub gaming_home_enabled: bool,
    pub suspend_bloat_on_boot: bool,
    pub guide_hotkey_bound: bool,
    pub suspended_processes: Vec<SuspendedBloatProcess>,
    pub previous_gamebar_nexus: Option<u32>,
    pub previous_auto_game_mode: Option<u32>,
    pub last_applied_ts: u64,
    pub status_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FseShellStatus {
    pub gaming_home_enabled: bool,
    pub cli_fse_booted: bool,
    pub suspend_bloat_on_boot: bool,
    pub guide_binding_active: bool,
    pub startup_command: String,
    pub suspended_count: usize,
    pub total_freed_mb: f64,
    pub suspended_processes: Vec<SuspendedBloatProcess>,
    pub platform_supported: bool,
    pub status_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FseBloatActionReport {
    pub action: String,
    pub affected_count: usize,
    pub total_freed_mb: f64,
    pub processes: Vec<SuspendedBloatProcess>,
    pub message: String,
}

fn io_lock() -> &'static Mutex<()> {
    FSE_IO_LOCK.get_or_init(|| Mutex::new(()))
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn init_cli_fse_flag(args: &[String]) -> bool {
    let active = args.iter().any(|a| a.eq_ignore_ascii_case("--fse"));
    let _ = FSE_CLI_ARG.set(active);
    active
}

pub fn is_cli_fse_booted() -> bool {
    *FSE_CLI_ARG.get().unwrap_or(&false)
}

pub fn is_fse_guide_binding_active() -> bool {
    FSE_GUIDE_ACTIVE.load(Ordering::Relaxed)
}

/// Safety guard ensuring critical OS, audio, DWM, and WebView2 processes are never touched.
pub fn is_safe_to_suspend_bloat_exe(exe_name: &str) -> bool {
    let low = exe_name.trim().to_ascii_lowercase();
    if low.is_empty() || PROTECTED_SYSTEM_EXES.contains(&low.as_str()) {
        return false;
    }
    NON_ESSENTIAL_BLOAT_EXES
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(&low))
}

pub fn build_fse_startup_command(exe_path: &std::path::Path) -> String {
    format!("\"{}\" --couch --fse", exe_path.display())
}

fn load_state_disk() -> FseShellState {
    let _guard = io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let Ok(path) = get_app_data_file_standalone(FSE_STATE_FILE) else {
        return FseShellState::default();
    };
    let Ok(raw) = fs::read_to_string(&path) else {
        return FseShellState::default();
    };
    serde_json::from_str::<FseShellState>(&raw).unwrap_or_default()
}

fn save_state_disk(state: &FseShellState) -> Result<(), String> {
    let _guard = io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let path = get_app_data_file_standalone(FSE_STATE_FILE)?;
    let raw = serde_json::to_string_pretty(state)
        .map_err(|e| format!("Failed serializing FSE state: {e}"))?;
    atomic_write_string(&path, &raw)
}

fn build_status_from_state(st: &FseShellState) -> FseShellStatus {
    let startup_cmd = std::env::current_exe()
        .map(|p| build_fse_startup_command(&p))
        .unwrap_or_else(|_| "\"Duke Launcher.exe\" --couch --fse".to_string());
    let total_freed_mb: f64 = st.suspended_processes.iter().map(|p| p.freed_mb).sum();
    FseShellStatus {
        gaming_home_enabled: st.gaming_home_enabled,
        cli_fse_booted: is_cli_fse_booted(),
        suspend_bloat_on_boot: st.suspend_bloat_on_boot,
        guide_binding_active: st.guide_hotkey_bound || st.gaming_home_enabled,
        startup_command: startup_cmd,
        suspended_count: st.suspended_processes.len(),
        total_freed_mb,
        suspended_processes: st.suspended_processes.clone(),
        platform_supported: cfg!(target_os = "windows"),
        status_summary: if st.status_summary.is_empty() {
            if st.gaming_home_enabled {
                "Duke Deck active as Windows Gaming Home (--couch --fse)".to_string()
            } else {
                "Standard desktop mode (1-click Gaming Home ready)".to_string()
            }
        } else {
            st.status_summary.clone()
        },
    }
}

/// Polls Windows `VK_GAMEPAD_VIEW` (`0xC6`) and `Win+G` edge-triggered chords when FSE Guide binding is active.
#[cfg(target_os = "windows")]
pub fn poll_fse_guide_chord_edge(prev_down: &mut bool) -> bool {
    if !is_fse_guide_binding_active() {
        *prev_down = false;
        return false;
    }
    const VK_GAMEPAD_VIEW: i32 = 0xC6;
    const VK_LWIN: i32 = 0x5B;
    const VK_G: i32 = 0x47;
    let down = duke_core::win32_ffi::is_vk_down(VK_GAMEPAD_VIEW)
        || (duke_core::win32_ffi::is_vk_down(VK_LWIN) && duke_core::win32_ffi::is_vk_down(VK_G));
    let triggered = down && !*prev_down;
    *prev_down = down;
    triggered
}

#[cfg(not(target_os = "windows"))]
pub fn poll_fse_guide_chord_edge(_prev_down: &mut bool) -> bool {
    false
}

fn suspend_bloat_processes_internal(state: &mut FseShellState) -> FseBloatActionReport {
    let mut newly_suspended = Vec::new();
    #[cfg(target_os = "windows")]
    {
        for exe_name in NON_ESSENTIAL_BLOAT_EXES {
            if !is_safe_to_suspend_bloat_exe(exe_name) {
                continue;
            }
            if let Some(pid) = duke_core::win32_ffi::find_pid_by_exe_name(exe_name) {
                if state.suspended_processes.iter().any(|p| p.pid == pid) {
                    continue;
                }
                if let Ok(freed_mb) = duke_core::win32_ffi::suspend_external_pid(pid) {
                    let rec = SuspendedBloatProcess {
                        pid,
                        exe_name: (*exe_name).to_string(),
                        freed_mb: freed_mb.max(0.0),
                    };
                    state.suspended_processes.push(rec.clone());
                    newly_suspended.push(rec);
                }
            }
        }
    }
    let total_freed: f64 = newly_suspended.iter().map(|p| p.freed_mb).sum();
    let count = newly_suspended.len();
    FseBloatActionReport {
        action: "suspend".to_string(),
        affected_count: count,
        total_freed_mb: total_freed,
        processes: newly_suspended,
        message: if count > 0 {
            format!(
                "Paused {count} non-essential Windows background bloat processes ({total_freed:.1} MB reclaimed)."
            )
        } else {
            "Zero active Windows bloat processes detected right now.".to_string()
        },
    }
}

fn resume_bloat_processes_internal(state: &mut FseShellState) -> FseBloatActionReport {
    let mut resumed = Vec::new();
    #[cfg(target_os = "windows")]
    {
        for proc_rec in state.suspended_processes.drain(..) {
            if duke_core::win32_ffi::resume_external_pid(proc_rec.pid).is_ok() {
                resumed.push(proc_rec);
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        state.suspended_processes.clear();
    }
    let count = resumed.len();
    FseBloatActionReport {
        action: "resume".to_string(),
        affected_count: count,
        total_freed_mb: 0.0,
        processes: resumed,
        message: format!("Resumed {count} background processes cleanly."),
    }
}

#[cfg(target_os = "windows")]
fn apply_windows_fse_registry(
    enable: bool,
    startup_cmd: &str,
    state: &mut FseShellState,
) -> Result<(), String> {
    if enable {
        if state.previous_gamebar_nexus.is_none() {
            state.previous_gamebar_nexus =
                duke_core::win32_ffi::reg_read_dword(false, REG_GAMEBAR_KEY, "UseNexusForGameBarEnabled");
        }
        if state.previous_auto_game_mode.is_none() {
            state.previous_auto_game_mode =
                duke_core::win32_ffi::reg_read_dword(false, REG_GAMEBAR_KEY, "AllowAutoGameMode");
        }
        duke_core::win32_ffi::reg_write_string(false, REG_RUN_KEY, REG_RUN_VALUE, startup_cmd)?;
        let _ = duke_core::win32_ffi::reg_write_dword(
            false,
            REG_GAMEBAR_KEY,
            "UseNexusForGameBarEnabled",
            0,
        );
        let _ =
            duke_core::win32_ffi::reg_write_dword(false, REG_GAMEBAR_KEY, "AllowAutoGameMode", 1);
        let _ =
            duke_core::win32_ffi::reg_write_dword(false, REG_DUKE_FSE_KEY, "GamingHomeActive", 1);
        let _ = duke_core::win32_ffi::reg_write_string(
            false,
            REG_DUKE_FSE_KEY,
            "ShellCommand",
            startup_cmd,
        );
    } else {
        let _ = duke_core::win32_ffi::reg_delete_value(false, REG_RUN_KEY, REG_RUN_VALUE);
        if let Some(prev) = state.previous_gamebar_nexus.take() {
            let _ = duke_core::win32_ffi::reg_write_dword(
                false,
                REG_GAMEBAR_KEY,
                "UseNexusForGameBarEnabled",
                prev,
            );
        }
        if let Some(prev) = state.previous_auto_game_mode.take() {
            let _ = duke_core::win32_ffi::reg_write_dword(
                false,
                REG_GAMEBAR_KEY,
                "AllowAutoGameMode",
                prev,
            );
        }
        let _ =
            duke_core::win32_ffi::reg_write_dword(false, REG_DUKE_FSE_KEY, "GamingHomeActive", 0);
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn apply_windows_fse_registry(
    _enable: bool,
    _startup_cmd: &str,
    _state: &mut FseShellState,
) -> Result<(), String> {
    Ok(())
}

/// Applies FSE Gaming Home boot automation on startup when `--fse` or saved toggle is active.
pub fn apply_fse_boot_sequence_if_enabled(app: &AppHandle) {
    let state = load_state_disk();
    let active = is_cli_fse_booted() || state.gaming_home_enabled;
    FSE_GUIDE_ACTIVE.store(active, Ordering::Release);
    if !active {
        return;
    }
    let app_c = app.clone();
    std::thread::Builder::new()
        .name("duke-fse-boot".to_string())
        .spawn(move || {
            let mut st = load_state_disk();
            if st.suspend_bloat_on_boot || is_cli_fse_booted() {
                let report = suspend_bloat_processes_internal(&mut st);
                st.status_summary = if report.affected_count > 0 {
                    format!("FSE Console Shell Active · {}", report.message)
                } else {
                    "FSE Console Shell Active (--couch --fse)".to_string()
                };
                st.last_applied_ts = now_unix_secs();
                let _ = save_state_disk(&st);
            }
            let status = build_status_from_state(&st);
            let _ = app_c.emit("fse-shell-updated", &status);
        })
        .ok();
}

#[tauri::command]
pub async fn get_fse_shell_status() -> Result<FseShellStatus, String> {
    let st = load_state_disk();
    FSE_GUIDE_ACTIVE.store(st.gaming_home_enabled || is_cli_fse_booted(), Ordering::Release);
    Ok(build_status_from_state(&st))
}

#[tauri::command]
pub async fn set_fse_gaming_home(
    app: AppHandle,
    enabled: bool,
    suspend_bloat: Option<bool>,
) -> Result<FseShellStatus, String> {
    let mut st = load_state_disk();
    let do_suspend = suspend_bloat.unwrap_or(true);
    let startup_cmd = std::env::current_exe()
        .map(|p| build_fse_startup_command(&p))
        .unwrap_or_else(|_| "\"Duke Launcher.exe\" --couch --fse".to_string());

    apply_windows_fse_registry(enabled, &startup_cmd, &mut st)?;
    st.gaming_home_enabled = enabled;
    st.suspend_bloat_on_boot = do_suspend;
    st.guide_hotkey_bound = enabled;
    st.last_applied_ts = now_unix_secs();
    FSE_GUIDE_ACTIVE.store(enabled, Ordering::Release);

    if enabled {
        if do_suspend {
            let report = suspend_bloat_processes_internal(&mut st);
            st.status_summary = format!(
                "Duke Deck set as Windows Gaming Home · {}",
                report.message
            );
        } else {
            st.status_summary = "Duke Deck set as Windows Gaming Home (--couch --fse)".to_string();
        }
        let mut settings = crate::load_settings_from_disk(&app);
        settings.couch_mode = true;
        let _ = crate::save_settings_to_disk(&app, &settings);
        let couch = crate::couch_mode::current_couch_status(true);
        let _ = app.emit("couch-mode-toggled", &couch);
    } else {
        let resume_report = resume_bloat_processes_internal(&mut st);
        st.status_summary = format!("Desktop mode restored · {}", resume_report.message);
    }

    save_state_disk(&st)?;
    let status = build_status_from_state(&st);
    let _ = app.emit("fse-shell-updated", &status);
    Ok(status)
}

#[tauri::command]
pub async fn suspend_windows_bloat_now(app: AppHandle) -> Result<FseBloatActionReport, String> {
    let mut st = load_state_disk();
    let report = suspend_bloat_processes_internal(&mut st);
    st.last_applied_ts = now_unix_secs();
    st.status_summary = report.message.clone();
    save_state_disk(&st)?;
    let _ = app.emit("fse-shell-updated", &build_status_from_state(&st));
    Ok(report)
}

#[tauri::command]
pub async fn resume_windows_bloat_now(app: AppHandle) -> Result<FseBloatActionReport, String> {
    let mut st = load_state_disk();
    let report = resume_bloat_processes_internal(&mut st);
    st.last_applied_ts = now_unix_secs();
    st.status_summary = report.message.clone();
    save_state_disk(&st)?;
    let _ = app.emit("fse-shell-updated", &build_status_from_state(&st));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn bloat_safety_guard_protects_critical_windows_processes() {
        assert!(!is_safe_to_suspend_bloat_exe("explorer.exe"));
        assert!(!is_safe_to_suspend_bloat_exe("dwm.exe"));
        assert!(!is_safe_to_suspend_bloat_exe("csrss.exe"));
        assert!(!is_safe_to_suspend_bloat_exe("lsass.exe"));
        assert!(!is_safe_to_suspend_bloat_exe("svchost.exe"));
        assert!(!is_safe_to_suspend_bloat_exe("duke-launcher.exe"));
        assert!(!is_safe_to_suspend_bloat_exe("msedgewebview2.exe"));
        assert!(is_safe_to_suspend_bloat_exe("OneDrive.exe"));
        assert!(is_safe_to_suspend_bloat_exe("Widgets.exe"));
    }

    #[test]
    fn startup_command_appends_couch_and_fse_flags() {
        let cmd = build_fse_startup_command(&PathBuf::from(r"C:\Apps\Duke Launcher.exe"));
        assert!(cmd.contains("--couch --fse"));
        assert!(cmd.starts_with('"'));
    }
}
