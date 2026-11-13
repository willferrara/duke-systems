//! Foreground window `.exe` detector and Win32 `SendInput` key/mouse macro injector.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
use windows::core::PWSTR;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{CloseHandle, HWND};
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, MOUSEINPUT,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL,
    MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSE_EVENT_FLAGS, VIRTUAL_KEY,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, SystemParametersInfoW, SPI_GETMOUSESPEED,
    SPI_SETMOUSESPEED, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemapActionType {
    Native,
    KeyTap,
    KeyCombo,
    MouseClick,
    RapidFire,
    DpiShiftHold,
    DpiStageCycle,
    GyroToggle,
    ProfileCycle,
    Macro,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemapBinding {
    pub source: String,
    pub action_type: RemapActionType,
    pub target: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MacroMode {
    Sequence,
    Hold,
    Toggle,
    RepeatWhileHeld,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MacroStepKind {
    KeyDown,
    KeyUp,
    KeyTap,
    MouseDown,
    MouseUp,
    MouseClick,
    DelayMs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroStep {
    pub kind: MacroStepKind,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroSequence {
    pub id: String,
    pub name: String,
    pub mode: MacroMode,
    pub repeat_delay_ms: u32,
    pub steps: Vec<MacroStep>,
}

/// Detects the foreground window's `(pid, exe_filename)` via `GetForegroundWindow` ->
/// `GetWindowThreadProcessId` -> `OpenProcess` -> `QueryFullProcessImageNameW`.
pub fn get_foreground_process_info() -> Option<(u32, String)> {
    #[cfg(target_os = "windows")]
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid: u32 = 0;
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let Ok(hproc) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return None;
        };
        if hproc.is_invalid() {
            return None;
        }
        let mut buf = [0u16; 260];
        let mut len = buf.len() as u32;
        let query_ok = QueryFullProcessImageNameW(
            hproc,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(hproc);
        if !query_ok || len == 0 {
            return None;
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        let exe_name = full
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if exe_name.is_empty() {
            None
        } else {
            Some((pid, exe_name))
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// Returns the foreground window's executable filename (e.g. `cs2.exe`), or empty string if unavailable.
pub fn get_foreground_exe_name() -> String {
    get_foreground_process_info()
        .map(|(_, exe)| exe)
        .unwrap_or_default()
}

pub fn exe_matches_rule(foreground_exe: &str, rule: &str) -> bool {
    let fg = foreground_exe.trim().to_ascii_lowercase();
    let rule_name = rule
        .trim()
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if fg.is_empty() || rule_name.is_empty() {
        return false;
    }
    fg == rule_name || fg.strip_suffix(".exe") == Some(rule_name.trim_end_matches(".exe"))
}

#[cfg(target_os = "windows")]
#[inline]
fn is_extended_vk(vk: u16) -> bool {
    matches!(
        vk,
        0x21..=0x28 | 0x2D..=0x2E | 0x5B..=0x5C | 0xA3 | 0xA5
    )
}

pub fn resolve_vk(name: &str) -> Option<u16> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() == 1 {
        let ch = trimmed.chars().next()?.to_ascii_uppercase();
        if ch.is_ascii_alphanumeric() {
            return Some(ch as u16);
        }
    }
    let upper = trimmed.to_ascii_uppercase();
    match upper.as_str() {
        "BACKSPACE" | "BACK" => Some(0x08),
        "TAB" => Some(0x09),
        "ENTER" | "RETURN" => Some(0x0D),
        "SHIFT" | "LSHIFT" => Some(0x10),
        "RSHIFT" => Some(0xA1),
        "CTRL" | "CONTROL" | "LCTRL" => Some(0x11),
        "RCTRL" => Some(0xA3),
        "ALT" | "MENU" | "LALT" => Some(0x12),
        "RALT" => Some(0xA5),
        "PAUSE" => Some(0x13),
        "CAPSLOCK" => Some(0x14),
        "ESC" | "ESCAPE" => Some(0x1B),
        "SPACE" | "SPACEBAR" => Some(0x20),
        "PGUP" | "PAGEUP" => Some(0x21),
        "PGDN" | "PAGEDOWN" => Some(0x22),
        "END" => Some(0x23),
        "HOME" => Some(0x24),
        "LEFT" | "ARROWLEFT" => Some(0x25),
        "UP" | "ARROWUP" => Some(0x26),
        "RIGHT" | "ARROWRIGHT" => Some(0x27),
        "DOWN" | "ARROWDOWN" => Some(0x28),
        "INS" | "INSERT" => Some(0x2D),
        "DEL" | "DELETE" => Some(0x2E),
        "WIN" | "LWIN" | "SUPER" => Some(0x5B),
        "RWIN" => Some(0x5C),
        "F1" => Some(0x70),
        "F2" => Some(0x71),
        "F3" => Some(0x72),
        "F4" => Some(0x73),
        "F5" => Some(0x74),
        "F6" => Some(0x75),
        "F7" => Some(0x76),
        "F8" => Some(0x77),
        "F9" => Some(0x78),
        "F10" => Some(0x79),
        "F11" => Some(0x7A),
        "F12" => Some(0x7B),
        "VOLUMEMUTE" | "MUTE" => Some(0xAD),
        "VOLUMEDOWN" => Some(0xAE),
        "VOLUMEUP" => Some(0xAF),
        "MEDIANEXT" => Some(0xB0),
        "MEDIAPREV" => Some(0xB1),
        "MEDIAPLAYPAUSE" => Some(0xB3),
        "MINUS" | "-" => Some(0xBD),
        "EQUALS" | "=" => Some(0xBB),
        "COMMA" | "," => Some(0xBC),
        "PERIOD" | "." => Some(0xBE),
        "SLASH" | "/" => Some(0xBF),
        "TILDE" | "`" => Some(0xC0),
        "LBRACKET" | "[" => Some(0xDB),
        "BACKSLASH" | "\\" => Some(0xDC),
        "RBRACKET" | "]" => Some(0xDD),
        "QUOTE" | "'" => Some(0xDE),
        _ => None,
    }
}

pub fn send_key_state(vk: u16, down: bool) {
    #[cfg(target_os = "windows")]
    unsafe {
        let scan = MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) as u16;
        let mut flags = KEYBD_EVENT_FLAGS(0);
        if is_extended_vk(vk) {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        if !down {
            flags |= KEYEVENTF_KEYUP;
        }
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0x4455_4B45,
                },
            },
        };
        let _ = SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = (vk, down);
}

pub fn send_key_tap(vk: u16) {
    send_key_state(vk, true);
    send_key_state(vk, false);
}

pub fn send_key_combo(combo: &str, down: bool) {
    let mut vks = [0u16; 6];
    let mut count = 0usize;
    for part in combo.split('+') {
        if count >= vks.len() {
            break;
        }
        if let Some(vk) = resolve_vk(part) {
            vks[count] = vk;
            count += 1;
        }
    }
    if count == 0 {
        return;
    }
    if down {
        for &vk in &vks[..count] {
            send_key_state(vk, true);
        }
    } else {
        for &vk in vks[..count].iter().rev() {
            send_key_state(vk, false);
        }
    }
}

#[inline]
pub fn send_mouse_move(dx: i32, dy: i32) {
    if dx == 0 && dy == 0 {
        return;
    }
    #[cfg(target_os = "windows")]
    unsafe {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE,
                    time: 0,
                    dwExtraInfo: 0x4455_4B45,
                },
            },
        };
        let _ = SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

pub fn send_mouse_button(button: &str, down: bool) {
    #[cfg(target_os = "windows")]
    {
        let upper = button.trim().to_ascii_uppercase();
        let (flags, data): (MOUSE_EVENT_FLAGS, u32) = match upper.as_str() {
            "MOUSELEFT" | "LEFT" | "LMB" => (
                if down {
                    MOUSEEVENTF_LEFTDOWN
                } else {
                    MOUSEEVENTF_LEFTUP
                },
                0,
            ),
            "MOUSERIGHT" | "RIGHT" | "RMB" => (
                if down {
                    MOUSEEVENTF_RIGHTDOWN
                } else {
                    MOUSEEVENTF_RIGHTUP
                },
                0,
            ),
            "MOUSEMIDDLE" | "MIDDLE" | "MMB" => (
                if down {
                    MOUSEEVENTF_MIDDLEDOWN
                } else {
                    MOUSEEVENTF_MIDDLEUP
                },
                0,
            ),
            "MOUSEX1" | "MOUSE4" | "XBUTTON1" => (
                if down {
                    MOUSEEVENTF_XDOWN
                } else {
                    MOUSEEVENTF_XUP
                },
                1,
            ),
            "MOUSEX2" | "MOUSE5" | "XBUTTON2" => (
                if down {
                    MOUSEEVENTF_XDOWN
                } else {
                    MOUSEEVENTF_XUP
                },
                2,
            ),
            "SCROLLUP" => {
                if !down {
                    return;
                }
                (MOUSEEVENTF_WHEEL, 120)
            }
            "SCROLLDOWN" => {
                if !down {
                    return;
                }
                (MOUSEEVENTF_WHEEL, (-120i32) as u32)
            }
            _ => return,
        };

        unsafe {
            let input = INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: data,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0x4455_4B45,
                    },
                },
            };
            let _ = SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = (button, down);
}

pub fn send_mouse_click(button: &str) {
    send_mouse_button(button, true);
    send_mouse_button(button, false);
}

#[derive(Debug, Clone)]
struct ActiveMacroSlot {
    macro_def: MacroSequence,
    step_idx: usize,
    next_step_at: Instant,
    physically_held: bool,
    toggled_on: bool,
    active: bool,
}

#[derive(Debug)]
pub struct InputActionEngine {
    prev_sources: Vec<(String, bool)>,
    rapid_fire_next: Instant,
    active_macros: Vec<ActiveMacroSlot>,
    baseline_win_mouse_speed: u32,
    last_applied_win_mouse_speed: u32,
    pub sniper_hold_active: bool,
}

impl Default for InputActionEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl InputActionEngine {
    pub fn new() -> Self {
        let baseline = get_windows_mouse_speed().unwrap_or(10);
        Self {
            prev_sources: Vec::with_capacity(32),
            rapid_fire_next: Instant::now(),
            active_macros: Vec::with_capacity(4),
            baseline_win_mouse_speed: baseline,
            last_applied_win_mouse_speed: baseline,
            sniper_hold_active: false,
        }
    }

    pub fn restore_windows_mouse_speed(&mut self) {
        if self.last_applied_win_mouse_speed != self.baseline_win_mouse_speed {
            let _ = set_windows_mouse_speed(self.baseline_win_mouse_speed);
            self.last_applied_win_mouse_speed = self.baseline_win_mouse_speed;
        }
        self.sniper_hold_active = false;
    }

    pub fn update_software_mouse_speed(&mut self, dpi_multiplier: f32, sniper_ratio: Option<f32>) {
        let eff_mult = if let Some(ratio) = sniper_ratio {
            (dpi_multiplier * ratio).clamp(0.1, 4.0)
        } else {
            dpi_multiplier.clamp(0.1, 4.0)
        };
        let target = ((self.baseline_win_mouse_speed as f32) * eff_mult)
            .round()
            .clamp(1.0, 20.0) as u32;
        if target != self.last_applied_win_mouse_speed {
            let _ = set_windows_mouse_speed(target);
            self.last_applied_win_mouse_speed = target;
        }
    }

    pub fn release_all_and_reset(&mut self) {
        for slot in self.active_macros.drain(..) {
            release_macro_held_steps(&slot.macro_def);
        }
        self.prev_sources.clear();
        self.restore_windows_mouse_speed();
    }

    /// Evaluates active remapping bindings against current source states and ticks active macros.
    /// Returns `(cycle_dpi_requested, cycle_profile_requested, gyro_toggle_edge_down)`.
    pub fn process_bindings(
        &mut self,
        now: Instant,
        bindings: &[RemapBinding],
        macros: &[MacroSequence],
        rapid_fire_cps: f32,
        source_states: &[(&str, bool)],
    ) -> (bool, bool, bool) {
        let mut cycle_dpi = false;
        let mut cycle_profile = false;
        let mut gyro_toggle_edge = false;
        let mut any_sniper_down = false;

        for binding in bindings {
            if binding.action_type == RemapActionType::Native
                || binding.action_type == RemapActionType::Disabled
            {
                continue;
            }

            let is_down = source_states
                .iter()
                .find(|(k, _)| *k == binding.source.as_str())
                .map(|(_, v)| *v)
                .unwrap_or(false);

            let prev_down = self.get_prev(&binding.source);
            self.set_prev(&binding.source, is_down);

            let rising = is_down && !prev_down;
            let falling = !is_down && prev_down;

            match binding.action_type {
                RemapActionType::KeyTap => {
                    if let Some(vk) = resolve_vk(&binding.target) {
                        if rising {
                            send_key_state(vk, true);
                        } else if falling {
                            send_key_state(vk, false);
                        }
                    }
                }
                RemapActionType::KeyCombo => {
                    if rising {
                        send_key_combo(&binding.target, true);
                    } else if falling {
                        send_key_combo(&binding.target, false);
                    }
                }
                RemapActionType::MouseClick => {
                    let btn = if binding.target.is_empty() {
                        "MouseLeft"
                    } else {
                        binding.target.as_str()
                    };
                    if rising {
                        send_mouse_button(btn, true);
                    } else if falling {
                        send_mouse_button(btn, false);
                    }
                }
                RemapActionType::RapidFire => {
                    if is_down && now >= self.rapid_fire_next {
                        let btn = if binding.target.is_empty() {
                            "MouseLeft"
                        } else {
                            binding.target.as_str()
                        };
                        send_mouse_click(btn);
                        let period_ms = (1000.0 / rapid_fire_cps.clamp(1.0, 40.0)).round() as u64;
                        self.rapid_fire_next = now + Duration::from_millis(period_ms.max(20));
                    }
                }
                RemapActionType::DpiShiftHold => {
                    if is_down {
                        any_sniper_down = true;
                    }
                }
                RemapActionType::DpiStageCycle => {
                    if rising {
                        cycle_dpi = true;
                    }
                }
                RemapActionType::GyroToggle => {
                    if rising {
                        gyro_toggle_edge = true;
                    }
                }
                RemapActionType::ProfileCycle => {
                    if rising {
                        cycle_profile = true;
                    }
                }
                RemapActionType::Macro => {
                    if let Some(macro_def) = macros.iter().find(|m| m.id == binding.target)
                        && (rising || falling) {
                            self.trigger_macro(macro_def, is_down, now);
                        }
                }
                RemapActionType::Native | RemapActionType::Disabled => {}
            }
        }

        self.sniper_hold_active = any_sniper_down;
        self.tick_macros(now);
        (cycle_dpi, cycle_profile, gyro_toggle_edge)
    }

    pub fn trigger_macro(&mut self, macro_def: &MacroSequence, edge_down: bool, now: Instant) {
        if macro_def.steps.is_empty() {
            return;
        }
        if let Some(slot) = self
            .active_macros
            .iter_mut()
            .find(|s| s.macro_def.id == macro_def.id)
        {
            slot.physically_held = edge_down;
            match macro_def.mode {
                MacroMode::Sequence => {
                    if edge_down {
                        slot.step_idx = 0;
                        slot.next_step_at = now;
                        slot.active = true;
                    }
                }
                MacroMode::Hold => {
                    if !edge_down {
                        release_macro_held_steps(&slot.macro_def);
                        slot.active = false;
                    } else {
                        slot.step_idx = 0;
                        slot.next_step_at = now;
                        slot.active = true;
                    }
                }
                MacroMode::Toggle => {
                    if edge_down {
                        slot.toggled_on = !slot.toggled_on;
                        if slot.toggled_on {
                            slot.step_idx = 0;
                            slot.next_step_at = now;
                            slot.active = true;
                        } else {
                            release_macro_held_steps(&slot.macro_def);
                            slot.active = false;
                        }
                    }
                }
                MacroMode::RepeatWhileHeld => {
                    if edge_down {
                        slot.step_idx = 0;
                        slot.next_step_at = now;
                        slot.active = true;
                    } else {
                        release_macro_held_steps(&slot.macro_def);
                        slot.active = false;
                    }
                }
            }
            return;
        }

        if !edge_down {
            return;
        }

        if self.active_macros.len() >= 8 {
            self.active_macros.remove(0);
        }
        self.active_macros.push(ActiveMacroSlot {
            macro_def: macro_def.clone(),
            step_idx: 0,
            next_step_at: now,
            physically_held: true,
            toggled_on: macro_def.mode == MacroMode::Toggle,
            active: true,
        });
    }

    pub fn tick_macros(&mut self, now: Instant) {
        for slot in &mut self.active_macros {
            if !slot.active {
                continue;
            }
            let mut guard = 0usize;
            while slot.active && now >= slot.next_step_at && guard < 16 {
                guard += 1;
                if slot.step_idx >= slot.macro_def.steps.len() {
                    match slot.macro_def.mode {
                        MacroMode::Sequence | MacroMode::Hold => {
                            slot.active = false;
                            break;
                        }
                        MacroMode::Toggle => {
                            if slot.toggled_on {
                                slot.step_idx = 0;
                                slot.next_step_at = now
                                    + Duration::from_millis(
                                        slot.macro_def.repeat_delay_ms.max(15) as u64,
                                    );
                                break;
                            } else {
                                slot.active = false;
                                break;
                            }
                        }
                        MacroMode::RepeatWhileHeld => {
                            if slot.physically_held {
                                slot.step_idx = 0;
                                slot.next_step_at = now
                                    + Duration::from_millis(
                                        slot.macro_def.repeat_delay_ms.max(15) as u64,
                                    );
                                break;
                            } else {
                                slot.active = false;
                                break;
                            }
                        }
                    }
                }

                let step = &slot.macro_def.steps[slot.step_idx];
                slot.step_idx += 1;
                match step.kind {
                    MacroStepKind::KeyDown => {
                        if let Some(vk) = resolve_vk(&step.value) {
                            send_key_state(vk, true);
                        }
                    }
                    MacroStepKind::KeyUp => {
                        if let Some(vk) = resolve_vk(&step.value) {
                            send_key_state(vk, false);
                        }
                    }
                    MacroStepKind::KeyTap => {
                        if step.value.contains('+') {
                            send_key_combo(&step.value, true);
                            send_key_combo(&step.value, false);
                        } else if let Some(vk) = resolve_vk(&step.value) {
                            send_key_tap(vk);
                        }
                    }
                    MacroStepKind::MouseDown => send_mouse_button(&step.value, true),
                    MacroStepKind::MouseUp => send_mouse_button(&step.value, false),
                    MacroStepKind::MouseClick => send_mouse_click(&step.value),
                    MacroStepKind::DelayMs => {
                        let ms = step.value.trim().parse::<u64>().unwrap_or(20).clamp(1, 5000);
                        slot.next_step_at = now + Duration::from_millis(ms);
                        break;
                    }
                }
            }
        }
        self.active_macros.retain(|s| s.active);
    }

    fn get_prev(&self, source: &str) -> bool {
        self.prev_sources
            .iter()
            .find(|(s, _)| s == source)
            .map(|(_, v)| *v)
            .unwrap_or(false)
    }

    fn set_prev(&mut self, source: &str, val: bool) {
        if let Some((_, existing)) = self.prev_sources.iter_mut().find(|(s, _)| s == source) {
            *existing = val;
        } else {
            self.prev_sources.push((source.to_string(), val));
        }
    }
}

fn release_macro_held_steps(macro_def: &MacroSequence) {
    for step in &macro_def.steps {
        match step.kind {
            MacroStepKind::KeyDown => {
                if let Some(vk) = resolve_vk(&step.value) {
                    send_key_state(vk, false);
                }
            }
            MacroStepKind::MouseDown => send_mouse_button(&step.value, false),
            _ => {}
        }
    }
}

pub fn get_windows_mouse_speed() -> Option<u32> {
    #[cfg(target_os = "windows")]
    unsafe {
        let mut speed: u32 = 10;
        if SystemParametersInfoW(
            SPI_GETMOUSESPEED,
            0,
            Some((&mut speed as *mut u32).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
        {
            return Some(speed.clamp(1, 20));
        }
    }
    None
}

pub fn set_windows_mouse_speed(speed: u32) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let clamped = speed.clamp(1, 20);
        unsafe {
            SystemParametersInfoW(
                SPI_SETMOUSESPEED,
                0,
                Some(clamped as usize as *mut core::ffi::c_void),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
            .map_err(|e| format!("SystemParametersInfoW(SPI_SETMOUSESPEED) failed: {}", e))
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = speed;
        Ok(())
    }
}
