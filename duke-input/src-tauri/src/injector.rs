pub use duke_core::macro_engine::{send_mouse_move, InputActionEngine};

use duke_core::shm_bus::ShmCoOpInputP2Payload;
use std::sync::{Mutex, OnceLock};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViGemXusbReport {
    pub w_buttons: u16,
    pub b_left_trigger: u8,
    pub b_right_trigger: u8,
    pub s_thumb_lx: i16,
    pub s_thumb_ly: i16,
    pub s_thumb_rx: i16,
    pub s_thumb_ry: i16,
}

impl From<ShmCoOpInputP2Payload> for ViGemXusbReport {
    fn from(pad: ShmCoOpInputP2Payload) -> Self {
        Self {
            w_buttons: pad.buttons,
            b_left_trigger: pad.left_trigger,
            b_right_trigger: pad.right_trigger,
            s_thumb_lx: pad.thumb_lx,
            s_thumb_ly: pad.thumb_ly,
            s_thumb_rx: pad.thumb_rx,
            s_thumb_ry: pad.thumb_ry,
        }
    }
}

#[cfg(target_os = "windows")]
mod vigem_rt {
    use super::ViGemXusbReport;
    use std::ffi::c_void;
    use std::sync::OnceLock;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    const VIGEM_ERROR_NONE: u32 = 0x2000_0000;

    type VigemAllocFn = unsafe extern "C" fn() -> *mut c_void;
    type VigemConnectFn = unsafe extern "C" fn(*mut c_void) -> u32;
    type VigemTargetAllocFn = unsafe extern "C" fn() -> *mut c_void;
    type VigemTargetAddFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32;
    type VigemX360UpdateFn =
        unsafe extern "C" fn(*mut c_void, *mut c_void, ViGemXusbReport) -> u32;

    pub(super) struct VigemClientSession {
        _module: HMODULE,
        client: *mut c_void,
        target: *mut c_void,
        update_fn: VigemX360UpdateFn,
    }

    unsafe impl Send for VigemClientSession {}
    unsafe impl Sync for VigemClientSession {}

    impl VigemClientSession {
        pub(super) fn submit(&self, report: ViGemXusbReport) -> bool {
            unsafe { (self.update_fn)(self.client, self.target, report) == VIGEM_ERROR_NONE }
        }
    }

    static VIGEM_SESSION: OnceLock<Option<VigemClientSession>> = OnceLock::new();

    pub(super) fn get_or_init() -> Option<&'static VigemClientSession> {
        VIGEM_SESSION
            .get_or_init(|| unsafe {
                let wide: Vec<u16> = "ViGEmClient.dll\0".encode_utf16().collect();
                let Ok(module) = LoadLibraryW(PCWSTR(wide.as_ptr())) else {
                    return None;
                };
                macro_rules! sym {
                    ($name:literal, $ty:ty) => {{
                        let s = concat!($name, "\0");
                        GetProcAddress(module, windows::core::PCSTR(s.as_ptr()))
                            .map(|p| std::mem::transmute::<_, $ty>(p))
                    }};
                }
                let alloc: VigemAllocFn = sym!("vigem_alloc", VigemAllocFn)?;
                let connect: VigemConnectFn = sym!("vigem_connect", VigemConnectFn)?;
                let target_alloc: VigemTargetAllocFn =
                    sym!("vigem_target_x360_alloc", VigemTargetAllocFn)?;
                let target_add: VigemTargetAddFn = sym!("vigem_target_add", VigemTargetAddFn)?;
                let update_fn: VigemX360UpdateFn =
                    sym!("vigem_target_x360_update", VigemX360UpdateFn)?;

                let client = alloc();
                if client.is_null() || connect(client) != VIGEM_ERROR_NONE {
                    return None;
                }
                let target = target_alloc();
                if target.is_null() || target_add(client, target) != VIGEM_ERROR_NONE {
                    return None;
                }
                Some(VigemClientSession {
                    _module: module,
                    client,
                    target,
                    update_fn,
                })
            })
            .as_ref()
    }
}

/// Computes `(virtual_key, pressed)` deltas so remote Player 2 XInput works in local co-op
/// even on hosts without ViGEmBus kernel drivers installed.
pub fn virtual_p2_keyboard_diff(
    prev: ShmCoOpInputP2Payload,
    next: ShmCoOpInputP2Payload,
) -> Vec<(u16, bool)> {
    // Canonical XInput bits:
    // DPAD U=0x0001 D=0x0002 L=0x0004 R=0x0008 START=0x0010 BACK=0x0020
    // LB=0x0100 RB=0x0200 A=0x1000 B=0x2000 X=0x4000 Y=0x8000
    // P2 non-conflicting bindings: Arrow keys + Numpad/IJKL cluster + Return/Backspace
    const MAP: &[(u16, u16)] = &[
        (0x0001, 0x26), // Up Arrow
        (0x0002, 0x28), // Down Arrow
        (0x0004, 0x25), // Left Arrow
        (0x0008, 0x27), // Right Arrow
        (0x1000, 0x4B), // A -> 'K'
        (0x2000, 0x4C), // B -> 'L'
        (0x4000, 0x4A), // X -> 'J'
        (0x8000, 0x49), // Y -> 'I'
        (0x0100, 0x55), // LB -> 'U'
        (0x0200, 0x4F), // RB -> 'O'
        (0x0010, 0x0D), // Start -> Enter
        (0x0020, 0x08), // Back -> Backspace
    ];

    let stick_mask = |pad: ShmCoOpInputP2Payload| -> u16 {
        let mut mask = pad.buttons;
        if pad.thumb_ly > 14_000 {
            mask |= 0x0001;
        } else if pad.thumb_ly < -14_000 {
            mask |= 0x0002;
        }
        if pad.thumb_lx < -14_000 {
            mask |= 0x0004;
        } else if pad.thumb_lx > 14_000 {
            mask |= 0x0008;
        }
        mask
    };

    let prev_mask = stick_mask(prev);
    let next_mask = stick_mask(next);
    let mut diffs = Vec::new();
    for &(bit, vk) in MAP {
        let was = (prev_mask & bit) != 0;
        let now = (next_mask & bit) != 0;
        if was != now {
            diffs.push((vk, now));
        }
    }
    diffs
}

static PREV_P2_STATE: OnceLock<Mutex<ShmCoOpInputP2Payload>> = OnceLock::new();

#[cfg(target_os = "windows")]
fn dispatch_vk_events(events: &[(u16, bool)]) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };
    if events.is_empty() {
        return;
    }
    let inputs: Vec<INPUT> = events
        .iter()
        .map(|&(vk, down)| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: if down {
                        Default::default()
                    } else {
                        KEYEVENTF_KEYUP
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        })
        .collect();
    unsafe {
        let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Injects remote Duke Link Player 2 gamepad state into the local host via ViGEm X360
/// (if `ViGEmClient.dll` is installed) or Win32 `SendInput` Player 2 key fallback.
pub fn inject_virtual_p2_xinput(state: ShmCoOpInputP2Payload) -> &'static str {
    #[cfg(target_os = "windows")]
    {
        if let Some(session) = vigem_rt::get_or_init() {
            if session.submit(ViGemXusbReport::from(state)) {
                return "ViGEm-X360";
            }
        }
        let lock = PREV_P2_STATE.get_or_init(|| Mutex::new(ShmCoOpInputP2Payload::default()));
        let diffs = if let Ok(mut prev) = lock.lock() {
            let d = virtual_p2_keyboard_diff(*prev, state);
            *prev = state;
            d
        } else {
            Vec::new()
        };
        dispatch_vk_events(&diffs);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = (&PREV_P2_STATE, state);
    "Win32-P2Fallback"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coop_p2_report_and_fallback_diff_work() {
        let prev = ShmCoOpInputP2Payload::default();
        let next = ShmCoOpInputP2Payload {
            buttons: 0x1000, // A button
            left_trigger: 64,
            right_trigger: 255,
            thumb_lx: 24000, // Right
            thumb_ly: 0,
            thumb_rx: -1200,
            thumb_ry: 3400,
            sequence: 7,
        };
        let report = ViGemXusbReport::from(next);
        assert_eq!(report.w_buttons, 0x1000);
        assert_eq!(report.b_right_trigger, 255);
        assert_eq!(report.s_thumb_lx, 24000);

        let diffs = virtual_p2_keyboard_diff(prev, next);
        assert!(diffs.contains(&(0x27, true))); // Right Arrow from thumb_lx
        assert!(diffs.contains(&(0x4B, true))); // 'K' from A button
    }
}
