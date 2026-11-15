use duke_core::hid_raw::{
    classify_hid_device, parse_fpv_hid_report, AxisCenterMode, ControllerAutoCalibrator,
    DeviceKind, FpvTelemetry,
};
use duke_core::hidapi::{HidApi, HidDevice};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::sync::RwLock;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

static COUCH_BOOT_ARG: OnceLock<bool> = OnceLock::new();
static RADIAL_VISIBLE: AtomicBool = AtomicBool::new(false);
static VIRTUAL_KEYBOARD_VISIBLE: AtomicBool = AtomicBool::new(false);
static CONTROLLER_KIND: OnceLock<RwLock<String>> = OnceLock::new();
static RADIAL_SELECTED_SLOT: OnceLock<RwLock<Option<u8>>> = OnceLock::new();
static HID_HUB: OnceLock<Mutex<CouchHidHub>> = OnceLock::new();

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

/// Unified controller snapshot combining Win32 XInput (`OnceLock`) and direct USB/BT HID
/// (`DualSense`, `DualShock 4`, `Switch Pro`, `8BitDo`, `EdgeTX` / `RadioMaster Pocket`, Generic HID).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UniversalPadState {
    pub connected: bool,
    pub slot: u8,
    pub lx: f32,
    pub ly: f32,
    pub rx: f32,
    pub ry: f32,
    pub lt: f32,
    pub rt: f32,
    pub a: bool,
    pub b: bool,
    pub x: bool,
    pub y: bool,
    pub lb: bool,
    pub rb: bool,
    pub back: bool,
    pub start: bool,
    pub ls: bool,
    pub rs: bool,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub guide: bool,
    pub buttons_mask: u16,
    pub controller_kind: String,
    pub device_name: String,
    pub vid_pid: String,
    pub raw_axes: [f32; 4],
    pub calibrated_axes: [f32; 4],
    pub axis_modes: [AxisCenterMode; 4],
    pub deadzone: f32,
    pub xinput_dll_ready: bool,
    pub hid_dll_ready: bool,
}

impl Default for UniversalPadState {
    fn default() -> Self {
        Self {
            connected: false,
            slot: 0,
            lx: 0.0,
            ly: 0.0,
            rx: 0.0,
            ry: 0.0,
            lt: 0.0,
            rt: 0.0,
            a: false,
            b: false,
            x: false,
            y: false,
            lb: false,
            rb: false,
            back: false,
            start: false,
            ls: false,
            rs: false,
            up: false,
            down: false,
            left: false,
            right: false,
            guide: false,
            buttons_mask: 0,
            controller_kind: "none".to_string(),
            device_name: "None".to_string(),
            vid_pid: String::new(),
            raw_axes: [0.0; 4],
            calibrated_axes: [0.0; 4],
            axis_modes: [AxisCenterMode::Centered; 4],
            deadzone: 0.14,
            xinput_dll_ready: false,
            hid_dll_ready: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerCalibrationStatus {
    pub deadzone: f32,
    pub center_offsets: [f32; 4],
    pub axis_modes: [AxisCenterMode; 4],
    pub raw_axes: [f32; 4],
    pub calibrated_axes: [f32; 4],
    pub controller_kind: String,
    pub device_name: String,
}

struct OpenedHidGamepad {
    device: HidDevice,
    kind: DeviceKind,
    vid: u16,
    pid: u16,
    name: String,
    fpv_min: [i32; 16],
    fpv_max: [i32; 16],
    fpv_tel: FpvTelemetry,
}

struct CouchHidHub {
    api: Option<HidApi>,
    api_failed: bool,
    last_scan: Option<Instant>,
    opened: Option<OpenedHidGamepad>,
    calibrator: ControllerAutoCalibrator,
    last_raw_axes: [f32; 4],
    last_hid_pad: Option<UniversalPadState>,
    last_kind: String,
    last_device_name: String,
}

impl CouchHidHub {
    fn new() -> Self {
        Self {
            api: None,
            api_failed: false,
            last_scan: None,
            opened: None,
            calibrator: ControllerAutoCalibrator::default(),
            last_raw_axes: [0.0; 4],
            last_hid_pad: None,
            last_kind: "none".to_string(),
            last_device_name: "None".to_string(),
        }
    }

    fn ensure_hid_device(&mut self) {
        if self.opened.is_some() || self.api_failed {
            return;
        }
        if !duke_core::win32_ffi::hid_dll_available() {
            return;
        }
        if let Some(prev) = self.last_scan {
            if prev.elapsed() < Duration::from_millis(1500) {
                return;
            }
        }
        self.last_scan = Some(Instant::now());

        if self.api.is_none() {
            match HidApi::new() {
                Ok(api) => self.api = Some(api),
                Err(_) => {
                    self.api_failed = true;
                    return;
                }
            }
        }

        let Some(ref mut api) = self.api else {
            return;
        };
        if api.refresh_devices().is_err() {
            return;
        }

        let mut seen_paths = HashSet::new();
        let mut best: Option<(u8, DeviceKind, u16, u16, String, std::ffi::CString)> = None;

        for info in api.device_list() {
            let up = info.usage_page();
            let usage = info.usage();
            // Strictly restrict HID probing to Gamepad (0x05) and Joystick (0x04) on Generic Desktop (0x01)
            // so mice, keyboards, and system vendor endpoints are never touched.
            if up != 0x01 || (usage != 0x04 && usage != 0x05) {
                continue;
            }
            let path_key = info.path().to_string_lossy().into_owned();
            if !seen_paths.insert(path_key) {
                continue;
            }
            let vid = info.vendor_id();
            let pid = info.product_id();
            let product = info
                .product_string()
                .unwrap_or("USB Gamepad")
                .trim()
                .to_string();
            let kind = classify_hid_device(vid, pid, up, usage, &product);
            if kind == DeviceKind::Xbox {
                continue;
            }
            let priority = match kind {
                DeviceKind::FpvTransmitter => 4,
                DeviceKind::PlayStation => 3,
                DeviceKind::SwitchPro => 2,
                _ => 1,
            };
            let take = match &best {
                None => true,
                Some((prev_p, ..)) => priority > *prev_p,
            };
            if take {
                best = Some((priority, kind, vid, pid, product, info.path().to_owned()));
            }
        }

        if let Some((_, kind, vid, pid, name, cpath)) = best {
            if let Ok(dev) = api.open_path(cpath.as_c_str()) {
                let _ = dev.set_blocking_mode(false);
                if kind == DeviceKind::FpvTransmitter {
                    self.calibrator.reset();
                }
                self.opened = Some(OpenedHidGamepad {
                    device: dev,
                    kind,
                    vid,
                    pid,
                    name,
                    fpv_min: [0; 16],
                    fpv_max: [2047; 16],
                    fpv_tel: FpvTelemetry::default(),
                });
            }
        }
    }
}

fn hid_hub() -> &'static Mutex<CouchHidHub> {
    HID_HUB.get_or_init(|| Mutex::new(CouchHidHub::new()))
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
    if low.contains("edgetx")
        || low.contains("opentx")
        || low.contains("radiomaster")
        || low.contains("pocket")
        || low.contains("fpv")
    {
        "edgetx"
    } else if low.contains("dualsense")
        || low.contains("dualshock")
        || low.contains("ps5")
        || low.contains("ps4")
        || low.contains("playstation")
    {
        "dualsense"
    } else if low.contains("switch") || low.contains("nintendo") || low.contains("8bitdo") {
        "switch"
    } else if low == "hid" || low.contains("generic") || low.contains("directinput") {
        "hid"
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

#[inline]
fn norm_u8_axis(v: u8) -> f32 {
    ((v as f32 - 127.5) / 127.5).clamp(-1.0, 1.0)
}

#[inline]
fn decode_hid_hat(nibble: u8) -> (bool, bool, bool, bool) {
    match nibble & 0x0F {
        0 => (true, false, false, false),
        1 => (true, false, false, true),
        2 => (false, false, false, true),
        3 => (false, true, false, true),
        4 => (false, true, false, false),
        5 => (false, true, true, false),
        6 => (false, false, true, false),
        7 => (true, false, true, false),
        _ => (false, false, false, false),
    }
}

fn parse_playstation_gamepad(pid: u16, buf: &[u8]) -> Option<UniversalPadState> {
    let report_id = *buf.first()?;
    let is_dualsense = matches!(pid, 0x0CE6 | 0x0DF2 | 0x0E5F);

    let (lx, ly, rx, ry, lt, rt, dpad_face, shoulders, misc) = if is_dualsense {
        if report_id == 0x01 && buf.len() >= 11 {
            (
                norm_u8_axis(buf[1]),
                -norm_u8_axis(buf[2]),
                norm_u8_axis(buf[3]),
                -norm_u8_axis(buf[4]),
                buf[5] as f32 / 255.0,
                buf[6] as f32 / 255.0,
                buf[8],
                buf[9],
                buf[10],
            )
        } else if report_id == 0x31 && buf.len() >= 12 {
            (
                norm_u8_axis(buf[2]),
                -norm_u8_axis(buf[3]),
                norm_u8_axis(buf[4]),
                -norm_u8_axis(buf[5]),
                buf[6] as f32 / 255.0,
                buf[7] as f32 / 255.0,
                buf[9],
                buf[10],
                buf[11],
            )
        } else {
            return None;
        }
    } else if report_id == 0x01 && buf.len() >= 8 {
        let lt_v = buf.get(8).copied().unwrap_or(0) as f32 / 255.0;
        let rt_v = buf.get(9).copied().unwrap_or(0) as f32 / 255.0;
        (
            norm_u8_axis(buf[1]),
            -norm_u8_axis(buf[2]),
            norm_u8_axis(buf[3]),
            -norm_u8_axis(buf[4]),
            lt_v,
            rt_v,
            buf[5],
            buf[6],
            buf[7],
        )
    } else if report_id == 0x11 && buf.len() >= 10 {
        let lt_v = buf.get(10).copied().unwrap_or(0) as f32 / 255.0;
        let rt_v = buf.get(11).copied().unwrap_or(0) as f32 / 255.0;
        (
            norm_u8_axis(buf[3]),
            -norm_u8_axis(buf[4]),
            norm_u8_axis(buf[5]),
            -norm_u8_axis(buf[6]),
            lt_v,
            rt_v,
            buf[7],
            buf[8],
            buf[9],
        )
    } else {
        return None;
    };

    let (up, down, left, right) = decode_hid_hat(dpad_face & 0x0F);
    Some(UniversalPadState {
        connected: true,
        lx,
        ly,
        rx,
        ry,
        lt,
        rt,
        a: (dpad_face & 0x20) != 0,
        b: (dpad_face & 0x40) != 0,
        x: (dpad_face & 0x10) != 0,
        y: (dpad_face & 0x80) != 0,
        lb: (shoulders & 0x01) != 0,
        rb: (shoulders & 0x02) != 0,
        back: (shoulders & 0x10) != 0,
        start: (shoulders & 0x20) != 0,
        ls: (shoulders & 0x40) != 0,
        rs: (shoulders & 0x80) != 0,
        up,
        down,
        left,
        right,
        guide: (misc & 0x01) != 0,
        controller_kind: "dualsense".to_string(),
        raw_axes: [lx, ly, rx, ry],
        ..Default::default()
    })
}

fn parse_switch_or_8bitdo_gamepad(buf: &[u8]) -> Option<UniversalPadState> {
    let report_id = *buf.first()?;
    if report_id == 0x30 && buf.len() >= 12 {
        let b3 = buf[3];
        let b4 = buf[4];
        let b5 = buf[5];
        let lx_raw = u16::from(buf[6]) | (u16::from(buf[7] & 0x0F) << 8);
        let ly_raw = u16::from(buf[7] >> 4) | (u16::from(buf[8]) << 4);
        let rx_raw = u16::from(buf[9]) | (u16::from(buf[10] & 0x0F) << 8);
        let ry_raw = u16::from(buf[10] >> 4) | (u16::from(buf[11]) << 4);
        let scale12 = |raw: u16| -> f32 { ((raw as f32 - 2048.0) / 1400.0).clamp(-1.0, 1.0) };
        let lx = scale12(lx_raw);
        let ly = scale12(ly_raw);
        let rx = scale12(rx_raw);
        let ry = scale12(ry_raw);
        return Some(UniversalPadState {
            connected: true,
            lx,
            ly,
            rx,
            ry,
            lt: if (b5 & 0x80) != 0 { 1.0 } else { 0.0 },
            rt: if (b3 & 0x80) != 0 { 1.0 } else { 0.0 },
            a: (b3 & 0x08) != 0 || (b3 & 0x04) != 0,
            b: (b3 & 0x04) != 0 && (b3 & 0x08) == 0,
            x: (b3 & 0x02) != 0,
            y: (b3 & 0x01) != 0,
            lb: (b5 & 0x40) != 0,
            rb: (b3 & 0x40) != 0,
            back: (b4 & 0x01) != 0,
            start: (b4 & 0x02) != 0,
            ls: (b4 & 0x08) != 0,
            rs: (b4 & 0x04) != 0,
            up: (b5 & 0x02) != 0,
            down: (b5 & 0x01) != 0,
            left: (b5 & 0x08) != 0,
            right: (b5 & 0x04) != 0,
            guide: (b4 & 0x10) != 0,
            controller_kind: "switch".to_string(),
            raw_axes: [lx, ly, rx, ry],
            ..Default::default()
        });
    }
    if report_id == 0x3F && buf.len() >= 12 {
        let b1 = buf[1];
        let b2 = buf[2];
        let (up, down, left, right) = decode_hid_hat(buf[3] & 0x0F);
        let lx = (u16::from_le_bytes([buf[4], buf[5]]) as f32 / 32767.5) - 1.0;
        let ly = -((u16::from_le_bytes([buf[6], buf[7]]) as f32 / 32767.5) - 1.0);
        let rx = (u16::from_le_bytes([buf[8], buf[9]]) as f32 / 32767.5) - 1.0;
        let ry = -((u16::from_le_bytes([buf[10], buf[11]]) as f32 / 32767.5) - 1.0);
        return Some(UniversalPadState {
            connected: true,
            lx: lx.clamp(-1.0, 1.0),
            ly: ly.clamp(-1.0, 1.0),
            rx: rx.clamp(-1.0, 1.0),
            ry: ry.clamp(-1.0, 1.0),
            lt: if (b1 & 0x40) != 0 { 1.0 } else { 0.0 },
            rt: if (b1 & 0x80) != 0 { 1.0 } else { 0.0 },
            a: (b1 & 0x01) != 0,
            b: (b1 & 0x02) != 0,
            x: (b1 & 0x04) != 0,
            y: (b1 & 0x08) != 0,
            lb: (b1 & 0x10) != 0,
            rb: (b1 & 0x20) != 0,
            back: (b2 & 0x01) != 0,
            start: (b2 & 0x02) != 0,
            ls: (b2 & 0x04) != 0,
            rs: (b2 & 0x08) != 0,
            up,
            down,
            left,
            right,
            guide: (b2 & 0x10) != 0,
            controller_kind: "switch".to_string(),
            raw_axes: [lx, ly, rx, ry],
            ..Default::default()
        });
    }
    None
}

fn parse_generic_hid_gamepad(buf: &[u8]) -> Option<UniversalPadState> {
    if buf.len() < 6 {
        return None;
    }
    let off = if buf[0] <= 0x04 && buf.len() >= 7 { 1 } else { 0 };
    let lx = norm_u8_axis(buf[off]);
    let ly = -norm_u8_axis(buf[off + 1]);
    let rx = norm_u8_axis(*buf.get(off + 2).unwrap_or(&128));
    let ry = -norm_u8_axis(*buf.get(off + 3).unwrap_or(&128));
    let b0 = *buf.get(off + 4).unwrap_or(&0);
    let b1 = *buf.get(off + 5).unwrap_or(&0);
    Some(UniversalPadState {
        connected: true,
        lx,
        ly,
        rx,
        ry,
        a: (b0 & 0x01) != 0,
        b: (b0 & 0x02) != 0,
        x: (b0 & 0x04) != 0,
        y: (b0 & 0x08) != 0,
        lb: (b0 & 0x10) != 0,
        rb: (b0 & 0x20) != 0,
        back: (b1 & 0x01) != 0,
        start: (b1 & 0x02) != 0,
        controller_kind: "hid".to_string(),
        raw_axes: [lx, ly, rx, ry],
        ..Default::default()
    })
}

/// Polls Xbox XInput first (`xinput1_4.dll` -> `xinput1_3.dll` -> `xinput9_1_0.dll`) and
/// seamlessly falls back to standalone USB/BT HID (`DualSense`, `DualShock 4`, `Switch Pro`,
/// `8BitDo`, `RadioMaster Pocket / EdgeTX VID_1209&PID_4F54`, or DirectInput HID) with axis auto-calibration.
pub fn poll_universal_controller_state() -> UniversalPadState {
    let xpad = duke_core::win32_ffi::poll_xinput_state();
    let hid_ready = duke_core::win32_ffi::hid_dll_available();

    let Ok(mut hub) = hid_hub().lock() else {
        return UniversalPadState::default();
    };

    if xpad.connected {
        let lx_norm = (f32::from(xpad.thumb_lx) / 32768.0).clamp(-1.0, 1.0);
        let ly_norm = (f32::from(xpad.thumb_ly) / 32768.0).clamp(-1.0, 1.0);
        let raw_axes = [lx_norm, ly_norm, 0.0, 0.0];
        let calibrated = hub.calibrator.normalize_axes(raw_axes, false);
        hub.last_raw_axes = raw_axes;
        hub.last_kind = "xbox".to_string();
        hub.last_device_name = "Xbox / XInput Controller (Slot 1)".to_string();

        let mut buttons_mask = 0u16;
        if xpad.dpad_up { buttons_mask |= 0x0001; }
        if xpad.dpad_down { buttons_mask |= 0x0002; }
        if xpad.dpad_left { buttons_mask |= 0x0004; }
        if xpad.dpad_right { buttons_mask |= 0x0008; }
        if xpad.start { buttons_mask |= 0x0010; }
        if xpad.back { buttons_mask |= 0x0020; }
        if xpad.lb { buttons_mask |= 0x0100; }
        if xpad.rb { buttons_mask |= 0x0200; }
        if xpad.a { buttons_mask |= 0x1000; }
        if xpad.b { buttons_mask |= 0x2000; }
        if xpad.x { buttons_mask |= 0x4000; }
        if xpad.y { buttons_mask |= 0x8000; }

        return UniversalPadState {
            connected: true,
            slot: 0,
            lx: calibrated[0],
            ly: calibrated[1],
            rx: calibrated[2],
            ry: calibrated[3],
            lt: 0.0,
            rt: 0.0,
            a: xpad.a,
            b: xpad.b,
            x: xpad.x,
            y: xpad.y,
            lb: xpad.lb,
            rb: xpad.rb,
            back: xpad.back,
            start: xpad.start,
            ls: false,
            rs: false,
            up: xpad.dpad_up,
            down: xpad.dpad_down,
            left: xpad.dpad_left,
            right: xpad.dpad_right,
            guide: xpad.guide,
            buttons_mask,
            controller_kind: "xbox".to_string(),
            device_name: hub.last_device_name.clone(),
            vid_pid: "XInput".to_string(),
            raw_axes,
            calibrated_axes: calibrated,
            axis_modes: hub.calibrator.axis_modes,
            deadzone: hub.calibrator.deadzone,
            xinput_dll_ready: true,
            hid_dll_ready: hid_ready,
        };
    }

    hub.ensure_hid_device();

    let mut latest_len = 0usize;
    let mut read_failed = false;
    let mut buf = [0u8; 64];

    if let Some(ref mut opened) = hub.opened {
        for _ in 0..4 {
            match opened.device.read(&mut buf) {
                Ok(n) if n > 0 => latest_len = n.min(64),
                Ok(_) => break,
                Err(_) => {
                    read_failed = true;
                    break;
                }
            }
        }
    }

    if read_failed {
        hub.opened = None;
        hub.last_hid_pad = None;
        hub.last_kind = "none".to_string();
        hub.last_device_name = "None".to_string();
        return UniversalPadState {
            hid_dll_ready: hid_ready,
            deadzone: hub.calibrator.deadzone,
            ..Default::default()
        };
    }

    if latest_len > 0 {
        let parsed = if let Some(ref mut opened) = hub.opened {
            match opened.kind {
                DeviceKind::FpvTransmitter => {
                    parse_fpv_hid_report(
                        &buf[..latest_len],
                        &mut opened.fpv_min,
                        &mut opened.fpv_max,
                        &mut opened.fpv_tel,
                    );
                    let ch = opened.fpv_tel.channels_raw;
                    // Mode-2 AETR mapping: [Yaw(ch3), Throttle(ch2), Roll(ch0), Pitch(ch1)].
                    // Centered gimbals drive smooth menu navigation without fighting resting low throttle.
                    let raw = [ch[3], ch[2], ch[0], ch[1]];
                    Some((
                        opened.kind,
                        opened.name.clone(),
                        format!("{:04X}:{:04X}", opened.vid, opened.pid),
                        UniversalPadState {
                            connected: true,
                            lx: raw[0],
                            ly: raw[1],
                            rx: raw[2],
                            ry: raw[3],
                            a: ch[4] > 0.35 || opened.fpv_tel.switches[0] >= 2,
                            b: ch[5] > 0.35 || opened.fpv_tel.switches[1] >= 2,
                            x: ch[6] > 0.35 || opened.fpv_tel.switches[2] >= 2,
                            y: ch[7] > 0.35 || opened.fpv_tel.switches[3] >= 2,
                            lb: ch[4] < -0.5,
                            rb: ch[5] < -0.5,
                            controller_kind: "edgetx".to_string(),
                            raw_axes: raw,
                            ..Default::default()
                        },
                    ))
                }
                DeviceKind::PlayStation => parse_playstation_gamepad(opened.pid, &buf[..latest_len])
                    .map(|p| {
                        (
                            opened.kind,
                            opened.name.clone(),
                            format!("{:04X}:{:04X}", opened.vid, opened.pid),
                            p,
                        )
                    }),
                DeviceKind::SwitchPro => {
                    let parsed = parse_switch_or_8bitdo_gamepad(&buf[..latest_len])
                        .or_else(|| parse_generic_hid_gamepad(&buf[..latest_len]));
                    parsed.map(|mut p| {
                        p.controller_kind = "switch".to_string();
                        (
                            opened.kind,
                            opened.name.clone(),
                            format!("{:04X}:{:04X}", opened.vid, opened.pid),
                            p,
                        )
                    })
                }
                _ => parse_generic_hid_gamepad(&buf[..latest_len]).map(|p| {
                    (
                        opened.kind,
                        opened.name.clone(),
                        format!("{:04X}:{:04X}", opened.vid, opened.pid),
                        p,
                    )
                }),
            }
        } else {
            None
        };

        if let Some((kind, dev_name, vid_pid, mut pad)) = parsed {
            let is_fpv = kind == DeviceKind::FpvTransmitter;
            let raw = pad.raw_axes;
            let calibrated = hub.calibrator.normalize_axes(raw, is_fpv);
            hub.last_raw_axes = raw;
            pad.lx = calibrated[0];
            pad.ly = calibrated[1];
            pad.rx = calibrated[2];
            pad.ry = calibrated[3];
            pad.calibrated_axes = calibrated;
            pad.axis_modes = hub.calibrator.axis_modes;
            pad.deadzone = hub.calibrator.deadzone;
            pad.device_name = dev_name.clone();
            pad.vid_pid = vid_pid;
            pad.hid_dll_ready = hid_ready;
            hub.last_kind = pad.controller_kind.clone();
            hub.last_device_name = dev_name;
            hub.last_hid_pad = Some(pad.clone());
            return pad;
        }
    }

    if hub.opened.is_some() {
        let is_fpv = hub
            .opened
            .as_ref()
            .map(|o| o.kind == DeviceKind::FpvTransmitter)
            .unwrap_or(false);
        if let Some(raw_axes) = hub.last_hid_pad.as_ref().map(|c| c.raw_axes) {
            let calibrated = hub.calibrator.normalize_axes(raw_axes, is_fpv);
            let axis_modes = hub.calibrator.axis_modes;
            let deadzone = hub.calibrator.deadzone;
            if let Some(ref mut cached) = hub.last_hid_pad {
                cached.lx = calibrated[0];
                cached.ly = calibrated[1];
                cached.rx = calibrated[2];
                cached.ry = calibrated[3];
                cached.calibrated_axes = calibrated;
                cached.axis_modes = axis_modes;
                cached.deadzone = deadzone;
                cached.hid_dll_ready = hid_ready;
                return cached.clone();
            }
        }
    }

    UniversalPadState {
        hid_dll_ready: hid_ready,
        deadzone: hub.calibrator.deadzone,
        ..Default::default()
    }
}

pub fn current_couch_status(settings_couch_mode: bool) -> CouchModeStatus {
    let pad = poll_universal_controller_state();
    let stored_kind = controller_kind_lock()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let kind = if pad.connected && pad.controller_kind != "none" {
        pad.controller_kind
    } else if stored_kind == "none" && pad.connected {
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

    let normalized_action = match action.trim() {
        "toggle_couch" => "toggle_couch_mode",
        "toggle_radial" => {
            if RADIAL_VISIBLE.load(Ordering::Relaxed) {
                "radial_close"
            } else {
                "radial_open"
            }
        }
        other => other,
    };

    match normalized_action {
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
            let status = current_couch_status(settings.couch_mode);
            let _ = app.emit("couch-radial-updated", &status);
            let _ = app.emit(
                "couch-radial-action",
                serde_json::json!({ "action": "toggle_virtual_keyboard", "slot": radial_slot }),
            );
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
            let _ = app.emit(
                "couch-radial-action",
                serde_json::json!({ "action": other, "slot": radial_slot }),
            );
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
    radial_open: Option<bool>,
) -> Result<CouchModeStatus, String> {
    if let Some(kind) = controller_kind {
        let norm = normalize_controller_kind(&kind);
        if norm != "none" {
            *controller_kind_lock()
                .write()
                .unwrap_or_else(|e| e.into_inner()) = norm.to_string();
        }
    }
    if let Some(ro) = radial_open {
        RADIAL_VISIBLE.store(ro, Ordering::Release);
        if !ro {
            *radial_slot_lock().write().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }
    let mut settings = crate::load_settings_from_disk(&app);
    settings.couch_mode = enabled;
    crate::save_settings_to_disk(&app, &settings)?;
    let status = current_couch_status(enabled);
    let _ = app.emit("couch-mode-toggled", &status);
    if radial_open.is_some() {
        let _ = app.emit("couch-radial-updated", &status);
    }
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
    let trimmed = action_id.trim();
    let resolved = if trimmed.is_empty() {
        slot.and_then(|s| {
            default_couch_radial_slots()
                .into_iter()
                .find(|item| item.slot == s)
                .map(|item| item.action_id)
        })
        .unwrap_or_default()
    } else {
        trimmed.to_string()
    };
    if !resolved.is_empty() {
        handle_controller_guide_event(&app, &resolved, &kind, slot);
    }
    let settings = crate::load_settings_from_disk(&app);
    Ok(current_couch_status(settings.couch_mode))
}

#[tauri::command]
pub fn calibrate_controller_axes(
    mode: String,
    deadzone: Option<f32>,
) -> Result<ControllerCalibrationStatus, String> {
    let pad = poll_universal_controller_state();
    let Ok(mut hub) = hid_hub().lock() else {
        return Err("Controller calibrator busy".to_string());
    };
    if let Some(dz) = deadzone {
        hub.calibrator.set_deadzone(dz);
    }
    match mode.trim().to_ascii_lowercase().as_str() {
        "auto_center" | "center" | "calibrate" => {
            hub.calibrator.capture_rest_pose(pad.raw_axes);
        }
        "reset" => {
            hub.calibrator.reset();
        }
        _ => {}
    }
    let is_fpv = hub.last_kind == "edgetx";
    let calibrated = hub.calibrator.normalize_axes(pad.raw_axes, is_fpv);
    Ok(ControllerCalibrationStatus {
        deadzone: hub.calibrator.deadzone,
        center_offsets: hub.calibrator.center_offsets,
        axis_modes: hub.calibrator.axis_modes,
        raw_axes: pad.raw_axes,
        calibrated_axes: calibrated,
        controller_kind: if pad.connected {
            pad.controller_kind
        } else {
            hub.last_kind.clone()
        },
        device_name: if pad.connected {
            pad.device_name
        } else {
            hub.last_device_name.clone()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_xbox_dualsense_and_edgetx_glyph_kinds() {
        assert_eq!(normalize_controller_kind("DualSense Wireless"), "dualsense");
        assert_eq!(normalize_controller_kind("PS5 Controller"), "dualsense");
        assert_eq!(normalize_controller_kind("Switch Pro"), "switch");
        assert_eq!(normalize_controller_kind("RadioMaster Pocket EdgeTX"), "edgetx");
        assert_eq!(normalize_controller_kind("Xbox Series X"), "xbox");
    }

    #[test]
    fn exposes_eight_radial_hotbar_slots() {
        let slots = default_couch_radial_slots();
        assert_eq!(slots.len(), 8);
        assert_eq!(slots[0].action_id, "toggle_couch_mode");
        assert_eq!(slots[7].action_id, "toggle_virtual_keyboard");
    }

    #[test]
    fn fpv_calibrator_separates_throttle_low_and_centered_gimbals() {
        let mut cal = ControllerAutoCalibrator::default();
        // Throttle on Mode-2 left Y (axis 1) resting at -0.96, gimbals centered near 0.02
        cal.capture_rest_pose([0.02, -0.96, -0.01, 0.03]);
        assert_eq!(cal.axis_modes[0], AxisCenterMode::Centered);
        assert_eq!(cal.axis_modes[1], AxisCenterMode::ThrottleLow);
        let resting = cal.normalize_axes([0.03, -0.95, 0.0, 0.02], true);
        assert!(resting.iter().all(|v| v.abs() < 0.001));
    }
}
