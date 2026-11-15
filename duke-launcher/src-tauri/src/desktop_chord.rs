//! Universal controller desktop chord layer via Win32 `SendInput`.
//!
//! Converts held controller chords (`Guide` on Xbox/PlayStation/Switch/DirectInput,
//! or `Guide` / `LB + RB` on `edgetx` transmitters such as RadioMaster Pocket) into
//! desktop mouse cursor movement, scroll wheel, left/right click, `Alt+Tab`, `Alt+F4`,
//! and system volume controls when Deck/Couch Mode is active.

use crate::couch_mode::UniversalPadState;
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Instant;

#[cfg(target_os = "windows")]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS,
    VIRTUAL_KEY, VK_F4, VK_MENU, VK_TAB, VK_VOLUME_DOWN, VK_VOLUME_MUTE, VK_VOLUME_UP,
};

const DEFAULT_DEADZONE: f32 = 0.14;
const TRIGGER_PRESS_THRESHOLD: f32 = 0.45;
const DEFAULT_FRAME_MS: u64 = 16;
const MAX_FRAME_DT_MS: u64 = 100;
const VOLUME_REPEAT_INTERVAL_MS: u64 = 180;
const MAX_CURSOR_PX_PER_SEC: f32 = 1150.0;
const CURSOR_LINEAR_BLEND: f32 = 0.15;
const MAX_SCROLL_DELTA_PER_SEC: f32 = 2400.0;
const SCROLL_LINEAR_BLEND: f32 = 0.20;

#[cfg(target_os = "windows")]
const DUKE_CHORD_EXTRA_INFO: usize = 0x4455_4B45;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DesktopChordAction {
    MoveCursor { dx: i32, dy: i32 },
    ScrollWheel { delta: i32 },
    LeftDown,
    LeftUp,
    RightDown,
    RightUp,
    AltTab,
    AltF4,
    VolumeUp,
    VolumeDown,
    VolumeMute,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopChordStatus {
    pub enabled: bool,
    pub chord_active: bool,
    pub actions_dispatched: u64,
    pub last_action: String,
}

impl Default for DesktopChordStatus {
    fn default() -> Self {
        Self {
            enabled: true,
            chord_active: false,
            actions_dispatched: 0,
            last_action: "none".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ChordTracker {
    pub enabled: bool,
    pub chord_active: bool,
    pub left_down: bool,
    pub right_down: bool,
    pub sensitivity: f32,
    prev_x: bool,
    prev_y: bool,
    prev_mute: bool,
    prev_up: bool,
    prev_down: bool,
    up_next_repeat_ms: u64,
    down_next_repeat_ms: u64,
    accum_dx: f32,
    accum_dy: f32,
    accum_scroll: f32,
    last_tick_ms: Option<u64>,
}

impl Default for ChordTracker {
    fn default() -> Self {
        Self {
            enabled: true,
            chord_active: false,
            left_down: false,
            right_down: false,
            sensitivity: 1.0,
            prev_x: false,
            prev_y: false,
            prev_mute: false,
            prev_up: false,
            prev_down: false,
            up_next_repeat_ms: 0,
            down_next_repeat_ms: 0,
            accum_dx: 0.0,
            accum_dy: 0.0,
            accum_scroll: 0.0,
            last_tick_ms: None,
        }
    }
}

#[derive(Debug)]
struct GlobalChordState {
    tracker: ChordTracker,
    actions_dispatched: u64,
    last_action: String,
}

impl Default for GlobalChordState {
    fn default() -> Self {
        Self {
            tracker: ChordTracker::default(),
            actions_dispatched: 0,
            last_action: "none".to_string(),
        }
    }
}

static CHORD_STATE: OnceLock<Mutex<GlobalChordState>> = OnceLock::new();
static CHORD_CLOCK_START: OnceLock<Instant> = OnceLock::new();

fn lock_global_state() -> MutexGuard<'static, GlobalChordState> {
    let lock = CHORD_STATE.get_or_init(|| Mutex::new(GlobalChordState::default()));
    match lock.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn monotonic_now_ms() -> u64 {
    let start = CHORD_CLOCK_START.get_or_init(Instant::now);
    let ms = start.elapsed().as_millis();
    u64::try_from(ms).unwrap_or(u64::MAX)
}

#[inline]
fn sanitize_axis(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

#[inline]
fn sanitize_trigger(v: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

#[inline]
fn sanitize_deadzone(raw: f32) -> f32 {
    if raw.is_finite() && raw > 0.0 {
        raw.clamp(0.01, 0.45)
    } else {
        DEFAULT_DEADZONE
    }
}

#[inline]
fn sanitize_sensitivity(raw: f32) -> f32 {
    if raw.is_finite() && raw > 0.0 {
        raw.clamp(0.1, 5.0)
    } else {
        1.0
    }
}

#[inline]
fn is_edgetx_controller(pad: &UniversalPadState) -> bool {
    pad.controller_kind.eq_ignore_ascii_case("edgetx")
}

/// Returns `true` when the controller is connected and its desktop chord activator is held:
/// `pad.guide` for standard gamepads, or `pad.guide || (pad.lb && pad.rb)` for `edgetx`.
#[inline]
pub(crate) fn is_chord_trigger_held(pad: &UniversalPadState) -> bool {
    if !pad.connected {
        return false;
    }
    if is_edgetx_controller(pad) {
        pad.guide || (pad.lb && pad.rb)
    } else {
        pad.guide
    }
}

fn release_held_state(tracker: &mut ChordTracker) -> Vec<DesktopChordAction> {
    let mut actions = Vec::new();
    if tracker.left_down {
        tracker.left_down = false;
        actions.push(DesktopChordAction::LeftUp);
    }
    if tracker.right_down {
        tracker.right_down = false;
        actions.push(DesktopChordAction::RightUp);
    }
    tracker.chord_active = false;
    tracker.prev_x = false;
    tracker.prev_y = false;
    tracker.prev_mute = false;
    tracker.prev_up = false;
    tracker.prev_down = false;
    tracker.up_next_repeat_ms = 0;
    tracker.down_next_repeat_ms = 0;
    tracker.accum_dx = 0.0;
    tracker.accum_dy = 0.0;
    tracker.accum_scroll = 0.0;
    tracker.last_tick_ms = None;
    actions
}

/// Pure deterministic state machine converting controller snapshots into desktop chord actions.
///
/// Automatically emits `LeftUp` / `RightUp` release actions whenever the chord is released,
/// Deck mode deactivates, desktop chords are disabled, or the controller disconnects.
pub(crate) fn compute_chord_actions(
    tracker: &mut ChordTracker,
    pad: &UniversalPadState,
    deck_active: bool,
    now_ms: u64,
) -> Vec<DesktopChordAction> {
    let active = tracker.enabled && deck_active && pad.connected && is_chord_trigger_held(pad);
    if !active {
        return release_held_state(tracker);
    }

    tracker.chord_active = true;

    let dt_ms = match tracker.last_tick_ms {
        None => DEFAULT_FRAME_MS,
        Some(prev) => now_ms.saturating_sub(prev).min(MAX_FRAME_DT_MS),
    };
    tracker.last_tick_ms = Some(now_ms);
    let dt_sec = dt_ms as f32 / 1000.0;

    let mut actions = Vec::new();
    let dz = sanitize_deadzone(pad.deadzone);
    let sensitivity = sanitize_sensitivity(tracker.sensitivity);

    // 1. Left stick -> smooth nonlinear cursor movement with fractional sub-pixel accumulation.
    let lx = sanitize_axis(pad.lx);
    let ly = sanitize_axis(pad.ly);
    let mag = (lx * lx + ly * ly).sqrt();
    if mag > dz {
        let norm_mag = ((mag - dz) / (1.0 - dz)).clamp(0.0, 1.0);
        let curved =
            CURSOR_LINEAR_BLEND * norm_mag + (1.0 - CURSOR_LINEAR_BLEND) * norm_mag * norm_mag;
        let speed = MAX_CURSOR_PX_PER_SEC * sensitivity * curved;
        let ux = lx / mag;
        let uy = ly / mag;

        tracker.accum_dx += ux * speed * dt_sec;
        // Controller +Y points up while Win32 screen +Y points down.
        tracker.accum_dy += -uy * speed * dt_sec;

        let dx = tracker.accum_dx.trunc() as i32;
        let dy = tracker.accum_dy.trunc() as i32;
        if dx != 0 || dy != 0 {
            tracker.accum_dx -= dx as f32;
            tracker.accum_dy -= dy as f32;
            actions.push(DesktopChordAction::MoveCursor { dx, dy });
        }
    } else {
        tracker.accum_dx = 0.0;
        tracker.accum_dy = 0.0;
    }

    // 2. Right stick Y -> smooth scroll wheel delta.
    let ry = sanitize_axis(pad.ry);
    let ry_abs = ry.abs();
    if ry_abs > dz {
        let norm_ry = ((ry_abs - dz) / (1.0 - dz)).clamp(0.0, 1.0);
        let curved_ry =
            SCROLL_LINEAR_BLEND * norm_ry + (1.0 - SCROLL_LINEAR_BLEND) * norm_ry * norm_ry;
        let scroll_rate = MAX_SCROLL_DELTA_PER_SEC * curved_ry;
        tracker.accum_scroll += ry.signum() * scroll_rate * dt_sec;

        let delta = tracker.accum_scroll.trunc() as i32;
        if delta != 0 {
            tracker.accum_scroll -= delta as f32;
            actions.push(DesktopChordAction::ScrollWheel { delta });
        }
    } else {
        tracker.accum_scroll = 0.0;
    }

    // 3. Mouse primary / secondary buttons (`RT` / `A` -> Left, `LT` / `B` -> Right).
    let left_pressed = sanitize_trigger(pad.rt) > TRIGGER_PRESS_THRESHOLD || pad.a;
    if left_pressed && !tracker.left_down {
        tracker.left_down = true;
        actions.push(DesktopChordAction::LeftDown);
    } else if !left_pressed && tracker.left_down {
        tracker.left_down = false;
        actions.push(DesktopChordAction::LeftUp);
    }

    let right_pressed = sanitize_trigger(pad.lt) > TRIGGER_PRESS_THRESHOLD || pad.b;
    if right_pressed && !tracker.right_down {
        tracker.right_down = true;
        actions.push(DesktopChordAction::RightDown);
    } else if !right_pressed && tracker.right_down {
        tracker.right_down = false;
        actions.push(DesktopChordAction::RightUp);
    }

    // 4. Window switcher & close rising-edge chords (`X` -> Alt+Tab, `Y` -> Alt+F4).
    if pad.x && !tracker.prev_x {
        actions.push(DesktopChordAction::AltTab);
    }
    tracker.prev_x = pad.x;

    if pad.y && !tracker.prev_y {
        actions.push(DesktopChordAction::AltF4);
    }
    tracker.prev_y = pad.y;

    // 5. Volume up / down with rising edge + 180ms auto-repeat.
    if pad.up {
        if !tracker.prev_up || now_ms >= tracker.up_next_repeat_ms {
            actions.push(DesktopChordAction::VolumeUp);
            tracker.up_next_repeat_ms = now_ms.saturating_add(VOLUME_REPEAT_INTERVAL_MS);
        }
    } else {
        tracker.up_next_repeat_ms = 0;
    }
    tracker.prev_up = pad.up;

    if pad.down {
        if !tracker.prev_down || now_ms >= tracker.down_next_repeat_ms {
            actions.push(DesktopChordAction::VolumeDown);
            tracker.down_next_repeat_ms = now_ms.saturating_add(VOLUME_REPEAT_INTERVAL_MS);
        }
    } else {
        tracker.down_next_repeat_ms = 0;
    }
    tracker.prev_down = pad.down;

    // 6. Volume mute (`RB` for standard gamepads, `LS` universally; avoids EdgeTX `LB+RB` trigger).
    let mute_pressed = if is_edgetx_controller(pad) {
        pad.ls
    } else {
        pad.rb || pad.ls
    };
    if mute_pressed && !tracker.prev_mute {
        actions.push(DesktopChordAction::VolumeMute);
    }
    tracker.prev_mute = mute_pressed;

    actions
}

fn describe_action(action: &DesktopChordAction) -> String {
    match action {
        DesktopChordAction::MoveCursor { .. } => "move_cursor",
        DesktopChordAction::ScrollWheel { .. } => "scroll_wheel",
        DesktopChordAction::LeftDown => "left_down",
        DesktopChordAction::LeftUp => "left_up",
        DesktopChordAction::RightDown => "right_down",
        DesktopChordAction::RightUp => "right_up",
        DesktopChordAction::AltTab => "alt_tab",
        DesktopChordAction::AltF4 => "alt_f4",
        DesktopChordAction::VolumeUp => "volume_up",
        DesktopChordAction::VolumeDown => "volume_down",
        DesktopChordAction::VolumeMute => "volume_mute",
    }
    .to_string()
}

fn snapshot_status(state: &GlobalChordState) -> DesktopChordStatus {
    DesktopChordStatus {
        enabled: state.tracker.enabled,
        chord_active: state.tracker.chord_active,
        actions_dispatched: state.actions_dispatched,
        last_action: state.last_action.clone(),
    }
}

pub(crate) fn set_desktop_chord_enabled(enabled: bool) -> DesktopChordStatus {
    let (cleanup_actions, status) = {
        let mut guard = lock_global_state();
        guard.tracker.enabled = enabled;
        let mut cleanup = Vec::new();
        if !enabled {
            cleanup = release_held_state(&mut guard.tracker);
            if !cleanup.is_empty() {
                guard.actions_dispatched = guard
                    .actions_dispatched
                    .saturating_add(cleanup.len() as u64);
                if let Some(last) = cleanup.last() {
                    guard.last_action = describe_action(last);
                }
            }
        }
        (cleanup, snapshot_status(&guard))
    };

    if !cleanup_actions.is_empty() {
        dispatch_chord_actions(&cleanup_actions);
    }

    status
}

pub(crate) fn get_desktop_chord_status() -> DesktopChordStatus {
    let guard = lock_global_state();
    snapshot_status(&guard)
}

/// Evaluates `pad` through the desktop chord layer and dispatches any emitted actions via Win32
/// `SendInput`. Returns `true` when the chord layer consumed this frame's input so the UI
/// suppresses normal gamepad navigation while the desktop chord is active.
pub(crate) fn process_pad_for_desktop_chord(pad: &UniversalPadState, deck_active: bool) -> bool {
    let now_ms = monotonic_now_ms();
    let (actions, consumed) = {
        let mut guard = lock_global_state();
        let actions = compute_chord_actions(&mut guard.tracker, pad, deck_active, now_ms);
        if !actions.is_empty() {
            guard.actions_dispatched = guard
                .actions_dispatched
                .saturating_add(actions.len() as u64);
            if let Some(last) = actions.last() {
                guard.last_action = describe_action(last);
            }
        }
        (actions, guard.tracker.chord_active)
    };

    if !actions.is_empty() {
        dispatch_chord_actions(&actions);
    }

    consumed
}

#[cfg(target_os = "windows")]
#[inline]
fn build_mouse_input(dx: i32, dy: i32, mouse_data: u32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: mouse_data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: DUKE_CHORD_EXTRA_INFO,
            },
        },
    }
}

#[cfg(target_os = "windows")]
#[inline]
fn build_key_input(vk: VIRTUAL_KEY, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if key_up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: DUKE_CHORD_EXTRA_INFO,
            },
        },
    }
}

#[cfg(target_os = "windows")]
fn actions_to_win32_inputs(actions: &[DesktopChordAction]) -> Vec<INPUT> {
    let mut inputs = Vec::with_capacity(actions.len().saturating_mul(2));
    for action in actions {
        match *action {
            DesktopChordAction::MoveCursor { dx, dy } => {
                if dx != 0 || dy != 0 {
                    inputs.push(build_mouse_input(dx, dy, 0, MOUSEEVENTF_MOVE));
                }
            }
            DesktopChordAction::ScrollWheel { delta } => {
                if delta != 0 {
                    inputs.push(build_mouse_input(0, 0, delta as u32, MOUSEEVENTF_WHEEL));
                }
            }
            DesktopChordAction::LeftDown => {
                inputs.push(build_mouse_input(0, 0, 0, MOUSEEVENTF_LEFTDOWN));
            }
            DesktopChordAction::LeftUp => {
                inputs.push(build_mouse_input(0, 0, 0, MOUSEEVENTF_LEFTUP));
            }
            DesktopChordAction::RightDown => {
                inputs.push(build_mouse_input(0, 0, 0, MOUSEEVENTF_RIGHTDOWN));
            }
            DesktopChordAction::RightUp => {
                inputs.push(build_mouse_input(0, 0, 0, MOUSEEVENTF_RIGHTUP));
            }
            DesktopChordAction::AltTab => {
                inputs.push(build_key_input(VK_MENU, false));
                inputs.push(build_key_input(VK_TAB, false));
                inputs.push(build_key_input(VK_TAB, true));
                inputs.push(build_key_input(VK_MENU, true));
            }
            DesktopChordAction::AltF4 => {
                inputs.push(build_key_input(VK_MENU, false));
                inputs.push(build_key_input(VK_F4, false));
                inputs.push(build_key_input(VK_F4, true));
                inputs.push(build_key_input(VK_MENU, true));
            }
            DesktopChordAction::VolumeUp => {
                inputs.push(build_key_input(VK_VOLUME_UP, false));
                inputs.push(build_key_input(VK_VOLUME_UP, true));
            }
            DesktopChordAction::VolumeDown => {
                inputs.push(build_key_input(VK_VOLUME_DOWN, false));
                inputs.push(build_key_input(VK_VOLUME_DOWN, true));
            }
            DesktopChordAction::VolumeMute => {
                inputs.push(build_key_input(VK_VOLUME_MUTE, false));
                inputs.push(build_key_input(VK_VOLUME_MUTE, true));
            }
        }
    }
    inputs
}

fn dispatch_chord_actions(actions: &[DesktopChordAction]) {
    #[cfg(target_os = "windows")]
    {
        let inputs = actions_to_win32_inputs(actions);
        if inputs.is_empty() {
            return;
        }
        if let Ok(cb_size) = i32::try_from(std::mem::size_of::<INPUT>()) {
            unsafe {
                let _ = SendInput(&inputs, cb_size);
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = actions;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xbox_pad() -> UniversalPadState {
        UniversalPadState {
            connected: true,
            controller_kind: "xbox".to_string(),
            deadzone: 0.14,
            ..Default::default()
        }
    }

    fn edgetx_pad() -> UniversalPadState {
        UniversalPadState {
            connected: true,
            controller_kind: "edgetx".to_string(),
            deadzone: 0.14,
            ..Default::default()
        }
    }

    #[test]
    fn chord_trigger_respects_xbox_and_edgetx_rules() {
        let mut xb = xbox_pad();
        assert!(!is_chord_trigger_held(&xb));
        xb.lb = true;
        xb.rb = true;
        assert!(!is_chord_trigger_held(&xb), "non-edgetx requires guide button");
        xb.guide = true;
        assert!(is_chord_trigger_held(&xb));
        xb.connected = false;
        assert!(!is_chord_trigger_held(&xb));

        let mut tx = edgetx_pad();
        assert!(!is_chord_trigger_held(&tx));
        tx.lb = true;
        assert!(!is_chord_trigger_held(&tx));
        tx.rb = true;
        assert!(is_chord_trigger_held(&tx), "edgetx supports lb+rb chord");
        tx.lb = false;
        tx.rb = false;
        tx.guide = true;
        assert!(is_chord_trigger_held(&tx), "edgetx also supports guide");
    }

    #[test]
    fn inactive_when_disabled_or_deck_inactive() {
        let mut tracker = ChordTracker::default();
        let mut pad = xbox_pad();
        pad.guide = true;
        pad.lx = 1.0;
        pad.x = true;

        let off_deck = compute_chord_actions(&mut tracker, &pad, false, 100);
        assert!(off_deck.is_empty());
        assert!(!tracker.chord_active);

        tracker.enabled = false;
        let disabled = compute_chord_actions(&mut tracker, &pad, true, 116);
        assert!(disabled.is_empty());
        assert!(!tracker.chord_active);
    }

    #[test]
    fn stick_cursor_deadzone_curve_and_fractional_accumulator() {
        let mut tracker = ChordTracker::default();
        let mut pad = xbox_pad();
        pad.guide = true;
        pad.lx = 0.08;
        pad.ly = 0.08;

        // Inside radial deadzone -> no movement.
        let inside = compute_chord_actions(&mut tracker, &pad, true, 100);
        assert!(inside.is_empty());

        // Gentle deflection just past deadzone -> accumulates sub-pixel movement smoothly.
        pad.lx = 0.18;
        pad.ly = 0.0;
        let mut total_dx = 0i32;
        let mut move_events = 0usize;
        for step in 1..=20 {
            let actions = compute_chord_actions(&mut tracker, &pad, true, 100 + step * 16);
            for act in actions {
                if let DesktopChordAction::MoveCursor { dx, dy } = act {
                    assert_eq!(dy, 0);
                    total_dx += dx;
                    move_events += 1;
                }
            }
        }
        assert!(total_dx >= 1 && (1..20).contains(&move_events));

        // Full tilt up (+Y) produces negative screen dy; right (+X) produces positive dx.
        let mut full_tracker = ChordTracker::default();
        pad.lx = 0.0;
        pad.ly = 1.0;
        let up_actions = compute_chord_actions(&mut full_tracker, &pad, true, 500);
        match up_actions.first() {
            Some(DesktopChordAction::MoveCursor { dx, dy }) => {
                assert_eq!(*dx, 0);
                assert!(*dy < -10, "expected negative screen dy when stick pushed up, got {dy}");
            }
            other => panic!("expected MoveCursor, got {other:?}"),
        }

        // Scroll wheel from right stick Y.
        pad.ly = 0.0;
        pad.ry = 1.0;
        let scroll_up = compute_chord_actions(&mut full_tracker, &pad, true, 516);
        assert!(matches!(
            scroll_up.as_slice(),
            [DesktopChordAction::ScrollWheel { delta }] if *delta > 0
        ));
    }

    #[test]
    fn left_right_clicks_and_release_on_chord_drop_or_disconnect() {
        let mut tracker = ChordTracker::default();
        let mut pad = xbox_pad();
        pad.guide = true;
        pad.rt = 0.8;
        pad.b = true;

        let press = compute_chord_actions(&mut tracker, &pad, true, 100);
        assert_eq!(
            press,
            vec![DesktopChordAction::LeftDown, DesktopChordAction::RightDown]
        );

        // Holding buttons across frames does not re-emit Down.
        let held = compute_chord_actions(&mut tracker, &pad, true, 116);
        assert!(held.is_empty());

        // Releasing triggers/buttons while chord stays held emits Up.
        pad.rt = 0.1;
        pad.b = false;
        let normal_release = compute_chord_actions(&mut tracker, &pad, true, 132);
        assert_eq!(
            normal_release,
            vec![DesktopChordAction::LeftUp, DesktopChordAction::RightUp]
        );

        // Re-press via A and LT, then drop Guide -> must emit LeftUp and RightUp automatically.
        pad.a = true;
        pad.lt = 0.9;
        let press_again = compute_chord_actions(&mut tracker, &pad, true, 148);
        assert_eq!(
            press_again,
            vec![DesktopChordAction::LeftDown, DesktopChordAction::RightDown]
        );

        pad.guide = false;
        let chord_drop = compute_chord_actions(&mut tracker, &pad, true, 164);
        assert_eq!(
            chord_drop,
            vec![DesktopChordAction::LeftUp, DesktopChordAction::RightUp]
        );
        assert!(!tracker.chord_active);

        // Press again then disconnect controller -> must emit LeftUp and RightUp automatically.
        pad.guide = true;
        let press_third = compute_chord_actions(&mut tracker, &pad, true, 180);
        assert_eq!(
            press_third,
            vec![DesktopChordAction::LeftDown, DesktopChordAction::RightDown]
        );

        pad.connected = false;
        let disconnect_release = compute_chord_actions(&mut tracker, &pad, true, 196);
        assert_eq!(
            disconnect_release,
            vec![DesktopChordAction::LeftUp, DesktopChordAction::RightUp]
        );
        assert!(!tracker.left_down && !tracker.right_down);
    }

    #[test]
    fn xbox_alt_tab_alt_f4_and_mute_edges() {
        let mut tracker = ChordTracker::default();
        let mut pad = xbox_pad();
        pad.guide = true;
        pad.x = true;
        pad.rb = true;

        let first = compute_chord_actions(&mut tracker, &pad, true, 100);
        assert_eq!(
            first,
            vec![DesktopChordAction::AltTab, DesktopChordAction::VolumeMute]
        );

        // Holding X/RB does not re-trigger on next frame.
        let held = compute_chord_actions(&mut tracker, &pad, true, 116);
        assert!(held.is_empty());

        // Press Y -> AltF4 on rising edge only.
        pad.x = false;
        pad.rb = false;
        pad.y = true;
        let second = compute_chord_actions(&mut tracker, &pad, true, 132);
        assert_eq!(second, vec![DesktopChordAction::AltF4]);

        let held_y = compute_chord_actions(&mut tracker, &pad, true, 148);
        assert!(held_y.is_empty());
    }

    #[test]
    fn edgetx_lb_rb_chord_volume_repeat_and_ls_mute() {
        let mut tracker = ChordTracker::default();
        let mut pad = edgetx_pad();
        pad.lb = true;
        pad.rb = true;
        pad.up = true;

        // LB+RB must NOT trigger VolumeMute on edgetx; Up fires immediately.
        let t0 = compute_chord_actions(&mut tracker, &pad, true, 1000);
        assert_eq!(t0, vec![DesktopChordAction::VolumeUp]);

        // Before 180ms -> no repeat.
        let t1 = compute_chord_actions(&mut tracker, &pad, true, 1100);
        assert!(t1.is_empty());

        // At 180ms -> repeats VolumeUp.
        let t2 = compute_chord_actions(&mut tracker, &pad, true, 1180);
        assert_eq!(t2, vec![DesktopChordAction::VolumeUp]);

        // Switch to DPad Down + LS mute.
        pad.up = false;
        pad.down = true;
        pad.ls = true;
        let t3 = compute_chord_actions(&mut tracker, &pad, true, 1200);
        assert_eq!(
            t3,
            vec![DesktopChordAction::VolumeDown, DesktopChordAction::VolumeMute]
        );

        let t4 = compute_chord_actions(&mut tracker, &pad, true, 1380);
        assert_eq!(t4, vec![DesktopChordAction::VolumeDown]);
    }

    #[test]
    fn global_api_idle_and_win32_input_translation_never_invokes_sendinput() {
        let disconnected = UniversalPadState::default();
        assert!(!process_pad_for_desktop_chord(&disconnected, false));

        let enabled_status = set_desktop_chord_enabled(true);
        assert!(enabled_status.enabled);
        let current = get_desktop_chord_status();
        assert!(current.enabled);

        #[cfg(target_os = "windows")]
        {
            let inputs = actions_to_win32_inputs(&[
                DesktopChordAction::MoveCursor { dx: 12, dy: -6 },
                DesktopChordAction::ScrollWheel { delta: 120 },
                DesktopChordAction::LeftDown,
                DesktopChordAction::LeftUp,
                DesktopChordAction::RightDown,
                DesktopChordAction::RightUp,
                DesktopChordAction::AltTab,
                DesktopChordAction::AltF4,
                DesktopChordAction::VolumeUp,
                DesktopChordAction::VolumeDown,
                DesktopChordAction::VolumeMute,
            ]);
            // 1 move + 1 wheel + 4 click + 4 alt-tab + 4 alt-f4 + 6 volume = 20 INPUT records.
            assert_eq!(inputs.len(), 20);
            unsafe {
                // Verify Alt+Tab & Alt+F4 both end with VK_MENU KEYEVENTF_KEYUP.
                assert_eq!(inputs[6].Anonymous.ki.wVk, VK_MENU);
                assert_eq!(inputs[6].Anonymous.ki.dwFlags, KEYBD_EVENT_FLAGS(0));
                assert_eq!(inputs[9].Anonymous.ki.wVk, VK_MENU);
                assert_eq!(inputs[9].Anonymous.ki.dwFlags, KEYEVENTF_KEYUP);
                assert_eq!(inputs[13].Anonymous.ki.wVk, VK_MENU);
                assert_eq!(inputs[13].Anonymous.ki.dwFlags, KEYEVENTF_KEYUP);
            }
        }
    }
}
