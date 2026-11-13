pub use duke_core::hid_raw::{
    classify_hid_device, parse_fpv_hid_report, DeviceKind, FpvTelemetry, HidDeviceSummary,
};
use hidapi::{HidApi, HidDevice};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::time::{Duration, Instant};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::XboxController::{
    XInputGetState, XInputSetState, XINPUT_STATE, XINPUT_VIBRATION,
};
use windows::Win32::UI::Input::{
    GetRawInputData, GetRawInputDeviceInfoW, GetRawInputDeviceList, RegisterRawInputDevices,
    HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTDEVICELIST, RAWINPUTHEADER, RIDEV_INPUTSINK,
    RIDI_DEVICENAME, RIDI_DEVICEINFO, RID_DEVICE_INFO, RID_INPUT, RIM_TYPEMOUSE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassExW,
    TranslateMessage, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_INPUT, WNDCLASSEXW,
};

// Lock-free RawInput mouse telemetry updated by the Win32 message thread
static RAW_MOUSE_LAST_NS: AtomicU64 = AtomicU64::new(0);
static RAW_MOUSE_EMA_DT_US: AtomicU32 = AtomicU32::new(0);
static RAW_MOUSE_JITTER_US: AtomicU32 = AtomicU32::new(0);
static RAW_MOUSE_POLL_HZ: AtomicU32 = AtomicU32::new(0);
static RAW_MOUSE_BUTTONS_MASK: AtomicU8 = AtomicU8::new(0);
static RAW_MOUSE_SCROLL_UP_PULSES: AtomicU32 = AtomicU32::new(0);
static RAW_MOUSE_SCROLL_DOWN_PULSES: AtomicU32 = AtomicU32::new(0);
static RAW_MOUSE_LAST_DX: AtomicI32 = AtomicI32::new(0);
static RAW_MOUSE_LAST_DY: AtomicI32 = AtomicI32::new(0);
static RAW_MOUSE_LAST_WHEEL: AtomicI32 = AtomicI32::new(0);
static RAW_INPUT_STARTED: AtomicBool = AtomicBool::new(false);

static START_ANCHOR: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

fn elapsed_ns() -> u64 {
    let anchor = START_ANCHOR.get_or_init(Instant::now);
    anchor.elapsed().as_nanos() as u64
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct XboxTelemetry {
    pub connected: bool,
    pub slot: u8,
    pub raw_lx: f32,
    pub raw_ly: f32,
    pub raw_rx: f32,
    pub raw_ry: f32,
    pub out_lx: f32,
    pub out_ly: f32,
    pub out_rx: f32,
    pub out_ry: f32,
    pub raw_lt: f32,
    pub raw_rt: f32,
    pub out_lt: f32,
    pub out_rt: f32,
    pub buttons_mask: u16,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MouseTelemetry {
    pub active_name: String,
    pub polling_hz: u32,
    pub jitter_us: u32,
    pub active_dpi_stage: usize,
    pub active_dpi: u16,
    pub sniper_active: bool,
    pub battery_pct: Option<u8>,
    pub battery_mv: Option<u16>,
    pub hidpp_supported: bool,
    pub buttons_mask: u8,
    pub last_dx: i32,
    pub last_dy: i32,
    pub wheel_delta: i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GyroTelemetry {
    pub connected: bool,
    pub name: String,
    pub active: bool,
    pub pitch_dps: f32,
    pub yaw_dps: f32,
    pub roll_dps: f32,
    pub accel_x: f32,
    pub accel_y: f32,
    pub accel_z: f32,
    pub mouse_dx: i32,
    pub mouse_dy: i32,
    pub touchpad_down: bool,
    pub battery_pct: Option<u8>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawHidInspectorData {
    pub device_id: String,
    pub device_name: String,
    pub vid_pid: String,
    pub report_len: usize,
    pub bytes: Vec<u8>,
    pub changed_mask: Vec<bool>,
    pub min_bytes: Vec<u8>,
    pub max_bytes: Vec<u8>,
    pub axes_u16: Vec<u16>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveTelemetrySnapshot {
    pub engine_hz: u32,
    pub engine_paused: bool,
    pub working_set_mb: f64,
    pub active_profile_id: String,
    pub active_profile_name: String,
    pub foreground_exe: String,
    pub auto_switch_enabled: bool,
    pub devices: Vec<HidDeviceSummary>,
    pub xbox: XboxTelemetry,
    pub mouse: MouseTelemetry,
    pub fpv: FpvTelemetry,
    pub gyro: GyroTelemetry,
    pub raw_inspector: RawHidInspectorData,
}

/// Raw Win32 mouse polling & button snapshot pulled without locking.
#[derive(Debug, Clone, Copy, Default)]
pub struct RawMouseSnapshot {
    pub polling_hz: u32,
    pub jitter_us: u32,
    pub buttons_mask: u8,
    pub scroll_up_pulses: u32,
    pub scroll_down_pulses: u32,
    pub last_dx: i32,
    pub last_dy: i32,
    pub last_wheel: i32,
}

#[inline]
pub fn read_raw_mouse_snapshot() -> RawMouseSnapshot {
    let last_ns = RAW_MOUSE_LAST_NS.load(Ordering::Relaxed);
    let now_ns = elapsed_ns();
    // Decay displayed Hz to 0 if no mouse movement/button packet arrived in > 1.5s
    let polling_hz = if now_ns.saturating_sub(last_ns) > 1_500_000_000 {
        0
    } else {
        RAW_MOUSE_POLL_HZ.load(Ordering::Relaxed)
    };
    RawMouseSnapshot {
        polling_hz,
        jitter_us: RAW_MOUSE_JITTER_US.load(Ordering::Relaxed),
        buttons_mask: RAW_MOUSE_BUTTONS_MASK.load(Ordering::Relaxed),
        scroll_up_pulses: RAW_MOUSE_SCROLL_UP_PULSES.swap(0, Ordering::Relaxed),
        scroll_down_pulses: RAW_MOUSE_SCROLL_DOWN_PULSES.swap(0, Ordering::Relaxed),
        last_dx: RAW_MOUSE_LAST_DX.swap(0, Ordering::Relaxed),
        last_dy: RAW_MOUSE_LAST_DY.swap(0, Ordering::Relaxed),
        last_wheel: RAW_MOUSE_LAST_WHEEL.swap(0, Ordering::Relaxed),
    }
}

pub fn start_raw_input_listener() {
    if RAW_INPUT_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    let _ = elapsed_ns();
    let _ = std::thread::Builder::new()
        .name("duke-rawinput".to_string())
        .spawn(|| unsafe {
            let class_name: Vec<u16> = "DukeInputRawMouseSink\0".encode_utf16().collect();
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(raw_wnd_proc),
                hInstance: hinst.into(),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };
            let _ = RegisterClassExW(&wc);
            let hwnd = match CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(class_name.as_ptr()),
                PCWSTR(class_name.as_ptr()),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(hinst.into()),
                None,
            ) {
                Ok(h) => h,
                Err(_) => return,
            };

            let rid = [RAWINPUTDEVICE {
                usUsagePage: 0x01,
                usUsage: 0x02,
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            }];
            let _ = RegisterRawInputDevices(&rid, std::mem::size_of::<RAWINPUTDEVICE>() as u32);

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
}

unsafe extern "system" fn raw_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_INPUT {
        let mut size: u32 = 0;
        let header_size = std::mem::size_of::<RAWINPUTHEADER>() as u32;
        let hraw = HRAWINPUT(lparam.0 as _);
        let _ = GetRawInputData(hraw, RID_INPUT, None, &mut size, header_size);
        if size > 0 && size <= 128 {
            let mut stack_buf = [0u8; 128];
            let read = GetRawInputData(
                hraw,
                RID_INPUT,
                Some(stack_buf.as_mut_ptr().cast()),
                &mut size,
                header_size,
            );
            if read != u32::MAX && (read as usize) >= std::mem::size_of::<RAWINPUTHEADER>() {
                let raw = &*(stack_buf.as_ptr() as *const RAWINPUT);
                if raw.header.dwType == RIM_TYPEMOUSE.0 {
                    let now_ns = elapsed_ns();
                    let prev_ns = RAW_MOUSE_LAST_NS.swap(now_ns, Ordering::Relaxed);
                    if prev_ns > 0 {
                        let dt_us = ((now_ns.saturating_sub(prev_ns)) / 1000) as u32;
                        if (200..=25_000).contains(&dt_us) {
                            let prev_ema = RAW_MOUSE_EMA_DT_US.load(Ordering::Relaxed);
                            let new_ema = if prev_ema == 0 {
                                dt_us
                            } else {
                                (prev_ema * 15 + dt_us) / 16
                            };
                            RAW_MOUSE_EMA_DT_US.store(new_ema, Ordering::Relaxed);
                            let jitter = dt_us.abs_diff(new_ema);
                            let prev_j = RAW_MOUSE_JITTER_US.load(Ordering::Relaxed);
                            RAW_MOUSE_JITTER_US.store((prev_j * 7 + jitter) / 8, Ordering::Relaxed);
                            if let Some(raw_hz) = 1_000_000u32.checked_div(new_ema) {
                                RAW_MOUSE_POLL_HZ.store(raw_hz.clamp(60, 8000), Ordering::Relaxed);
                            }
                        }
                    }

                    let mouse = raw.data.mouse;
                    RAW_MOUSE_LAST_DX.fetch_add(mouse.lLastX, Ordering::Relaxed);
                    RAW_MOUSE_LAST_DY.fetch_add(mouse.lLastY, Ordering::Relaxed);

                    let flags = mouse.Anonymous.Anonymous.usButtonFlags;
                    let mut mask = RAW_MOUSE_BUTTONS_MASK.load(Ordering::Relaxed);
                    if flags & 0x0001 != 0 {
                        mask |= 1 << 0;
                    }
                    if flags & 0x0002 != 0 {
                        mask &= !(1 << 0);
                    }
                    if flags & 0x0004 != 0 {
                        mask |= 1 << 1;
                    }
                    if flags & 0x0008 != 0 {
                        mask &= !(1 << 1);
                    }
                    if flags & 0x0010 != 0 {
                        mask |= 1 << 2;
                    }
                    if flags & 0x0020 != 0 {
                        mask &= !(1 << 2);
                    }
                    if flags & 0x0040 != 0 {
                        mask |= 1 << 3;
                    }
                    if flags & 0x0080 != 0 {
                        mask &= !(1 << 3);
                    }
                    if flags & 0x0100 != 0 {
                        mask |= 1 << 4;
                    }
                    if flags & 0x0200 != 0 {
                        mask &= !(1 << 4);
                    }
                    RAW_MOUSE_BUTTONS_MASK.store(mask, Ordering::Relaxed);

                    if flags & 0x0400 != 0 {
                        let wheel = mouse.Anonymous.Anonymous.usButtonData as i16 as i32;
                        RAW_MOUSE_LAST_WHEEL.store(wheel, Ordering::Relaxed);
                        if wheel > 0 {
                            RAW_MOUSE_SCROLL_UP_PULSES.fetch_add(1, Ordering::Relaxed);
                        } else if wheel < 0 {
                            RAW_MOUSE_SCROLL_DOWN_PULSES.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
        }
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

#[inline]
fn normalize_i16_axis(val: i16) -> f32 {
    if val < 0 {
        (val as f32 / 32768.0).clamp(-1.0, 1.0)
    } else {
        (val as f32 / 32767.0).clamp(-1.0, 1.0)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct XInputRawState {
    pub connected: bool,
    pub slot: u8,
    pub lx: f32,
    pub ly: f32,
    pub rx: f32,
    pub ry: f32,
    pub lt: f32,
    pub rt: f32,
    pub buttons_mask: u16,
}

/// Polls XInput slots `0..4`. Disconnected slots are probed periodically to eliminate XInput overhead.
pub fn poll_xinput(probe_disconnected: bool, active_mask: &mut [bool; 4]) -> XInputRawState {
    let mut state = XINPUT_STATE::default();
    for (slot, active) in active_mask.iter_mut().enumerate() {
        if !*active && !probe_disconnected {
            continue;
        }
        let res = unsafe { XInputGetState(slot as u32, &mut state) };
        if res == 0 {
            *active = true;
            let gp = state.Gamepad;
            return XInputRawState {
                connected: true,
                slot: slot as u8,
                lx: normalize_i16_axis(gp.sThumbLX),
                ly: normalize_i16_axis(gp.sThumbLY),
                rx: normalize_i16_axis(gp.sThumbRX),
                ry: normalize_i16_axis(gp.sThumbRY),
                lt: (gp.bLeftTrigger as f32 / 255.0).clamp(0.0, 1.0),
                rt: (gp.bRightTrigger as f32 / 255.0).clamp(0.0, 1.0),
                buttons_mask: gp.wButtons.0,
            };
        } else {
            *active = false;
        }
    }
    XInputRawState::default()
}

pub fn set_xinput_rumble(slot: u8, left_pct: f32, right_pct: f32) -> Result<(), String> {
    let left_speed = (left_pct.clamp(0.0, 1.0) * 65535.0).round() as u16;
    let right_speed = (right_pct.clamp(0.0, 1.0) * 65535.0).round() as u16;
    let vib = XINPUT_VIBRATION {
        wLeftMotorSpeed: left_speed,
        wRightMotorSpeed: right_speed,
    };
    let rc = unsafe { XInputSetState(slot as u32, &vib) };
    if rc == 0 {
        Ok(())
    } else {
        Err(format!("XInput slot {} not connected (code {})", slot, rc))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GyroImuFrame {
    pub valid: bool,
    pub pitch_dps: f32,
    pub yaw_dps: f32,
    pub roll_dps: f32,
    pub accel_x: f32,
    pub accel_y: f32,
    pub accel_z: f32,
    pub l2_down: bool,
    pub r2_down: bool,
    pub touchpad_down: bool,
    pub touchpad_click: bool,
    pub battery_pct: Option<u8>,
}

#[derive(Debug)]
struct OpenedHidHandle {
    id: String,
    kind: DeviceKind,
    summary: HidDeviceSummary,
    device: HidDevice,
    hidpp_dev_idx: u8,
    hidpp_batt_feat: u8,
    hidpp_is_unified_batt: bool,
    hidpp_dpi_feat: u8,
    fpv_min: [i32; 16],
    fpv_max: [i32; 16],
}

pub struct HidDeviceManager {
    api: Option<HidApi>,
    opened: Vec<OpenedHidHandle>,
    raw_mice_names: Vec<(String, u16, u16)>,
    last_scan: Instant,
    last_batt_poll: Instant,
    inspector_target_id: String,
    inspector_buf: [u8; 64],
    inspector_len: usize,
    inspector_min: [u8; 64],
    inspector_max: [u8; 64],
    inspector_change_times: [Option<Instant>; 64],
}

impl HidDeviceManager {
    pub fn new() -> Self {
        let api = HidApi::new().ok();
        let mut mgr = Self {
            api,
            opened: Vec::with_capacity(8),
            raw_mice_names: Vec::new(),
            last_scan: Instant::now() - Duration::from_secs(10),
            last_batt_poll: Instant::now() - Duration::from_secs(60),
            inspector_target_id: String::new(),
            inspector_buf: [0u8; 64],
            inspector_len: 0,
            inspector_min: [0xFFu8; 64],
            inspector_max: [0u8; 64],
            inspector_change_times: [None; 64],
        };
        mgr.rescan_devices();
        mgr
    }

    pub fn set_inspector_target(&mut self, device_id: &str) {
        if self.inspector_target_id != device_id {
            self.inspector_target_id = device_id.to_string();
            self.reset_inspector_bounds();
        }
    }

    pub fn reset_inspector_bounds(&mut self) {
        self.inspector_min = [0xFFu8; 64];
        self.inspector_max = [0u8; 64];
        self.inspector_change_times = [None; 64];
        self.inspector_len = 0;
    }

    pub fn maybe_rescan(&mut self) {
        if self.last_scan.elapsed() >= Duration::from_millis(1600) {
            self.rescan_devices();
        }
    }

    pub fn rescan_devices(&mut self) {
        self.last_scan = Instant::now();
        self.raw_mice_names = enumerate_raw_mice();

        let Some(api) = self.api.as_mut() else {
            return;
        };
        if api.refresh_devices().is_err() {
            return;
        }

        let mut next_opened: Vec<OpenedHidHandle> = Vec::with_capacity(8);

        for info in api.device_list() {
            let vid = info.vendor_id();
            let pid = info.product_id();
            let up = info.usage_page();
            let usage = info.usage();
            let iface = info.interface_number();
            let product = info
                .product_string()
                .unwrap_or("HID Controller / Peripheral")
                .trim()
                .to_string();
            let path_str = info.path().to_string_lossy().to_string();
            let id = format!("hid-{:04x}-{:04x}-{}-{}", vid, pid, iface, up);

            let kind = classify_hid_device(vid, pid, up, usage, &product);
            let is_interesting = matches!(
                kind,
                DeviceKind::LogitechMouse
                    | DeviceKind::FpvTransmitter
                    | DeviceKind::PlayStation
                    | DeviceKind::SwitchPro
                    | DeviceKind::Xbox
            ) || (up == 0x01 && (usage == 0x04 || usage == 0x05));

            if !is_interesting {
                continue;
            }
            if next_opened.iter().any(|h| h.id == id) {
                continue;
            }

            // Reuse existing handle if already open
            if let Some(pos) = self.opened.iter().position(|h| h.id == id) {
                next_opened.push(self.opened.remove(pos));
                continue;
            }

            if let Ok(device) = api.open_path(info.path()) {
                let _ = device.set_blocking_mode(false);
                let transport = if path_str.to_ascii_lowercase().contains("bth")
                    || path_str.contains("{00001124")
                {
                    "Bluetooth".to_string()
                } else if vid == 0x046D && (pid == 0xC53F || pid == 0xC548 || pid == 0xC52B) {
                    "Lightspeed / Receiver".to_string()
                } else {
                    "USB".to_string()
                };

                let mut handle = OpenedHidHandle {
                    id: id.clone(),
                    kind,
                    summary: HidDeviceSummary {
                        id,
                        kind,
                        class_name: match kind {
                            DeviceKind::Xbox => "Xbox HID".to_string(),
                            DeviceKind::LogitechMouse => "Logitech HID++".to_string(),
                            DeviceKind::RawMouse => "Raw Mouse".to_string(),
                            DeviceKind::FpvTransmitter => "FPV Transmitter".to_string(),
                            DeviceKind::PlayStation => "PlayStation IMU".to_string(),
                            DeviceKind::SwitchPro => "Switch Pro IMU".to_string(),
                            DeviceKind::GenericHid => "Raw HID".to_string(),
                        },
                        name: product,
                        vid,
                        pid,
                        usage_page: up,
                        usage,
                        interface_number: iface,
                        transport,
                        battery_pct: None,
                        battery_mv: None,
                        polling_hz: None,
                        supports_gyro: matches!(
                            kind,
                            DeviceKind::PlayStation | DeviceKind::SwitchPro
                        ),
                        supports_hidpp: kind == DeviceKind::LogitechMouse
                            && (up == 0xFF00 || up == 0xFF01),
                        supports_fpv: kind == DeviceKind::FpvTransmitter,
                        supports_rumble: matches!(
                            kind,
                            DeviceKind::Xbox | DeviceKind::PlayStation
                        ),
                    },
                    device,
                    hidpp_dev_idx: 0xFF,
                    hidpp_batt_feat: 0,
                    hidpp_is_unified_batt: false,
                    hidpp_dpi_feat: 0,
                    fpv_min: [0; 16],
                    fpv_max: [2047; 16],
                };

                if handle.summary.supports_hidpp {
                    probe_logitech_hidpp_features(&mut handle);
                } else if handle.kind == DeviceKind::SwitchPro {
                    enable_switch_pro_imu(&handle.device);
                }

                next_opened.push(handle);
            }
        }

        self.opened = next_opened;
    }

    pub fn poll_all(
        &mut self,
        buf: &mut [u8; 64],
        inspect_active: bool,
        fpv_out: &mut FpvTelemetry,
        gyro_out: &mut GyroImuFrame,
        gyro_name: &mut String,
    ) {
        if self.last_batt_poll.elapsed() >= Duration::from_secs(20) {
            self.last_batt_poll = Instant::now();
            for handle in &mut self.opened {
                if handle.summary.supports_hidpp && handle.hidpp_batt_feat != 0 {
                    query_logitech_battery(handle);
                }
            }
        }

        let target_id = if self.inspector_target_id.is_empty() {
            self.opened.first().map(|h| h.id.as_str()).unwrap_or("")
        } else {
            self.inspector_target_id.as_str()
        }
        .to_string();

        for handle in &mut self.opened {
            let mut latest_len = 0usize;
            // Drain buffered HID packets so we always operate on the freshest 1 ms frame
            for _ in 0..4 {
                match handle.device.read(buf) {
                    Ok(n) if n > 0 => {
                        latest_len = n.min(64);
                    }
                    _ => break,
                }
            }

            if latest_len == 0 {
                continue;
            }

            if inspect_active && handle.id == target_id {
                let now = Instant::now();
                self.inspector_len = latest_len;
                for (i, &b) in buf.iter().enumerate().take(latest_len) {
                    if b != self.inspector_buf[i] {
                        self.inspector_change_times[i] = Some(now);
                    }
                    self.inspector_buf[i] = b;
                    if b < self.inspector_min[i] {
                        self.inspector_min[i] = b;
                    }
                    if b > self.inspector_max[i] {
                        self.inspector_max[i] = b;
                    }
                }
            }

            match handle.kind {
                DeviceKind::FpvTransmitter => {
                    parse_fpv_hid_report(
                        &buf[..latest_len],
                        &mut handle.fpv_min,
                        &mut handle.fpv_max,
                        fpv_out,
                    );
                    fpv_out.connected = true;
                    fpv_out.name = handle.summary.name.clone();
                }
                DeviceKind::PlayStation => {
                    let parsed = parse_playstation_report(
                        handle.summary.pid,
                        &buf[..latest_len],
                    );
                    if parsed.valid {
                        *gyro_out = parsed;
                        if let Some(pct) = parsed.battery_pct {
                            handle.summary.battery_pct = Some(pct);
                        }
                        *gyro_name = handle.summary.name.clone();
                    }
                }
                DeviceKind::SwitchPro => {
                    let parsed = parse_switch_pro_report(&buf[..latest_len]);
                    if parsed.valid {
                        *gyro_out = parsed;
                        *gyro_name = handle.summary.name.clone();
                    }
                }
                _ => {}
            }
        }
    }

    pub fn apply_logitech_dpi(&mut self, dpi: u16) -> bool {
        let mut applied = false;
        for handle in &self.opened {
            if handle.summary.supports_hidpp && handle.hidpp_dpi_feat != 0 {
                let hi = (dpi >> 8) as u8;
                let lo = (dpi & 0xFF) as u8;
                let mut pkt = [0u8; 20];
                pkt[0] = 0x11;
                pkt[1] = handle.hidpp_dev_idx;
                pkt[2] = handle.hidpp_dpi_feat;
                pkt[3] = 0x30; // setSensorDpi (func 3)
                pkt[4] = 0x00; // sensor 0
                pkt[5] = hi;
                pkt[6] = lo;
                if handle.device.write(&pkt).is_ok() {
                    applied = true;
                }
            }
        }
        applied
    }

    pub fn build_device_summaries(
        &self,
        xbox_connected: bool,
        xbox_slot: u8,
        raw_mouse_hz: u32,
    ) -> (Vec<HidDeviceSummary>, Option<u8>, Option<u16>, bool, String) {
        let mut out = Vec::with_capacity(self.opened.len() + 2);
        let mut logi_batt = None;
        let mut logi_mv = None;
        let mut logi_hidpp = false;
        let mut active_mouse_name = String::new();

        if xbox_connected {
            out.push(HidDeviceSummary {
                id: format!("xinput-{}", xbox_slot),
                kind: DeviceKind::Xbox,
                class_name: "Xbox / XInput".to_string(),
                name: format!("Xbox / XInput Controller (Slot {})", xbox_slot + 1),
                vid: 0x045E,
                pid: 0x02FF,
                usage_page: 0x01,
                usage: 0x05,
                interface_number: xbox_slot as i32,
                transport: "XInput 1.4".to_string(),
                battery_pct: None,
                battery_mv: None,
                polling_hz: Some(1000),
                supports_gyro: false,
                supports_hidpp: false,
                supports_fpv: false,
                supports_rumble: true,
            });
        }

        for h in &self.opened {
            let mut s = h.summary.clone();
            if h.kind == DeviceKind::LogitechMouse {
                if s.polling_hz.is_none() && raw_mouse_hz > 0 {
                    s.polling_hz = Some(raw_mouse_hz);
                }
                if s.supports_hidpp {
                    logi_hidpp = true;
                }
                if s.battery_pct.is_some() {
                    logi_batt = s.battery_pct;
                    logi_mv = s.battery_mv;
                }
                if active_mouse_name.is_empty() {
                    active_mouse_name = s.name.clone();
                }
            }
            out.push(s);
        }

        for (idx, (mname, vid, pid)) in self.raw_mice_names.iter().enumerate() {
            let already = out.iter().any(|d| {
                d.kind == DeviceKind::LogitechMouse
                    || (d.vid == *vid && d.pid == *pid && *vid != 0)
            });
            if !already {
                if active_mouse_name.is_empty() {
                    active_mouse_name = mname.clone();
                }
                out.push(HidDeviceSummary {
                    id: format!("rawmouse-{}", idx),
                    kind: DeviceKind::RawMouse,
                    class_name: "RawInput Mouse".to_string(),
                    name: mname.clone(),
                    vid: *vid,
                    pid: *pid,
                    usage_page: 0x01,
                    usage: 0x02,
                    interface_number: 0,
                    transport: "Win32 RawInput".to_string(),
                    battery_pct: None,
                    battery_mv: None,
                    polling_hz: if raw_mouse_hz > 0 {
                        Some(raw_mouse_hz)
                    } else {
                        None
                    },
                    supports_gyro: false,
                    supports_hidpp: false,
                    supports_fpv: false,
                    supports_rumble: false,
                });
            }
        }

        if active_mouse_name.is_empty() {
            active_mouse_name = "System Mouse (RawInput)".to_string();
        }

        (out, logi_batt, logi_mv, logi_hidpp, active_mouse_name)
    }

    pub fn build_inspector_snapshot(&self) -> RawHidInspectorData {
        let target = if self.inspector_target_id.is_empty() {
            self.opened.first()
        } else {
            self.opened
                .iter()
                .find(|h| h.id == self.inspector_target_id)
                .or_else(|| self.opened.first())
        };

        let Some(handle) = target else {
            return RawHidInspectorData::default();
        };

        let len = self.inspector_len.min(64);
        let now = Instant::now();
        let mut changed = Vec::with_capacity(len);
        for i in 0..len {
            let recent = self.inspector_change_times[i]
                .map(|t| now.duration_since(t) <= Duration::from_millis(500))
                .unwrap_or(false);
            changed.push(recent);
        }

        let mut axes_u16 = Vec::with_capacity(len / 2);
        for chunk in self.inspector_buf[..len].chunks_exact(2) {
            axes_u16.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }

        RawHidInspectorData {
            device_id: handle.id.clone(),
            device_name: handle.summary.name.clone(),
            vid_pid: format!("{:04X}:{:04X}", handle.summary.vid, handle.summary.pid),
            report_len: len,
            bytes: self.inspector_buf[..len].to_vec(),
            changed_mask: changed,
            min_bytes: self.inspector_min[..len].to_vec(),
            max_bytes: self.inspector_max[..len].to_vec(),
            axes_u16,
        }
    }
}

fn hidpp_get_feature(device: &HidDevice, dev_idx: u8, feature_id: u16) -> Option<u8> {
    let mut pkt = [0u8; 20];
    pkt[0] = 0x11;
    pkt[1] = dev_idx;
    pkt[2] = 0x00; // IRoot
    pkt[3] = 0x01; // GetFeature
    pkt[4] = (feature_id >> 8) as u8;
    pkt[5] = (feature_id & 0xFF) as u8;

    let mut short_pkt = [0u8; 7];
    short_pkt.copy_from_slice(&pkt[..7]);

    let _ = device.write(&pkt);
    let _ = device.write(&short_pkt);

    let mut resp = [0u8; 32];
    for _ in 0..3 {
        if let Ok(n) = device.read_timeout(&mut resp, 15) {
            if n >= 6
                && (resp[0] == 0x10 || resp[0] == 0x11)
                && resp[1] == dev_idx
                && resp[2] == 0x00
                && resp[4] != 0
            {
                return Some(resp[4]);
            }
        }
    }
    None
}

fn probe_logitech_hidpp_features(handle: &mut OpenedHidHandle) {
    // Wired/Bluetooth uses 0xFF; Lightspeed/Unifying receivers use slots 0x01..0x02
    for dev_idx in [0xFFu8, 0x01u8, 0x02u8] {
        if let Some(feat) = hidpp_get_feature(&handle.device, dev_idx, 0x1004) {
            handle.hidpp_dev_idx = dev_idx;
            handle.hidpp_batt_feat = feat;
            handle.hidpp_is_unified_batt = true;
            break;
        }
        if let Some(feat) = hidpp_get_feature(&handle.device, dev_idx, 0x1000) {
            handle.hidpp_dev_idx = dev_idx;
            handle.hidpp_batt_feat = feat;
            handle.hidpp_is_unified_batt = false;
            break;
        }
    }
    if handle.hidpp_batt_feat != 0 {
        handle.hidpp_dpi_feat =
            hidpp_get_feature(&handle.device, handle.hidpp_dev_idx, 0x2201).unwrap_or(0);
        query_logitech_battery(handle);
    } else if let Some(dpi_feat) = hidpp_get_feature(&handle.device, 0xFF, 0x2201) {
        handle.hidpp_dev_idx = 0xFF;
        handle.hidpp_dpi_feat = dpi_feat;
    }
}

fn query_logitech_battery(handle: &mut OpenedHidHandle) {
    if handle.hidpp_batt_feat == 0 {
        return;
    }
    let mut pkt = [0u8; 20];
    pkt[0] = 0x11;
    pkt[1] = handle.hidpp_dev_idx;
    pkt[2] = handle.hidpp_batt_feat;
    pkt[3] = if handle.hidpp_is_unified_batt {
        0x10 // GetStatus for 0x1004
    } else {
        0x00 // GetBatteryLevelStatus for 0x1000
    };
    let _ = handle.device.write(&pkt);

    let mut resp = [0u8; 32];
    for _ in 0..3 {
        if let Ok(n) = handle.device.read_timeout(&mut resp, 15) {
            if n >= 7
                && (resp[0] == 0x10 || resp[0] == 0x11)
                && resp[1] == handle.hidpp_dev_idx
                && resp[2] == handle.hidpp_batt_feat
            {
                let pct = resp[4];
                if pct <= 100 {
                    handle.summary.battery_pct = Some(pct);
                }
                if !handle.hidpp_is_unified_batt && n >= 7 {
                    // HID++ 0x1000 sometimes reports next-level % in resp[5]
                    if handle.summary.battery_pct.is_none() && resp[5] <= 100 {
                        handle.summary.battery_pct = Some(resp[5]);
                    }
                }
                // Voltage read via 0x1001 if supported or approximated from LiPo curve
                if let Some(pct) = handle.summary.battery_pct {
                    let est_mv = 3300 + ((pct as u16) * 9);
                    handle.summary.battery_mv = Some(est_mv);
                }
                break;
            }
        }
    }
}

fn enable_switch_pro_imu(device: &HidDevice) {
    let _ = device.write(&[0x80, 0x02]);
    let _ = device.write(&[0x80, 0x04]);
    // Subcommand 0x40 = Enable IMU (0x01)
    let mut cmd = [0u8; 49];
    cmd[0] = 0x01;
    cmd[1] = 0x01;
    cmd[10] = 0x40;
    cmd[11] = 0x01;
    let _ = device.write(&cmd);
}

fn parse_playstation_report(pid: u16, buf: &[u8]) -> GyroImuFrame {
    let is_dualsense = pid == 0x0CE6 || pid == 0x0DF2;
    let mut frame = GyroImuFrame::default();

    if is_dualsense {
        let base = if buf[0] == 0x01 && buf.len() >= 48 {
            1usize
        } else if buf[0] == 0x31 && buf.len() >= 50 {
            2usize
        } else {
            return frame;
        };

        let gx = i16::from_le_bytes([buf[base + 15], buf[base + 16]]) as f32;
        let gy = i16::from_le_bytes([buf[base + 17], buf[base + 18]]) as f32;
        let gz = i16::from_le_bytes([buf[base + 19], buf[base + 20]]) as f32;
        let ax = i16::from_le_bytes([buf[base + 21], buf[base + 22]]) as f32;
        let ay = i16::from_le_bytes([buf[base + 23], buf[base + 24]]) as f32;
        let az = i16::from_le_bytes([buf[base + 25], buf[base + 26]]) as f32;

        frame.valid = true;
        frame.pitch_dps = gx / 16.4;
        frame.yaw_dps = -gy / 16.4;
        frame.roll_dps = gz / 16.4;
        frame.accel_x = ax / 8192.0;
        frame.accel_y = ay / 8192.0;
        frame.accel_z = az / 8192.0;
        frame.l2_down = buf[base + 4] > 40;
        frame.r2_down = buf[base + 5] > 40;
        if buf.len() > base + 33 {
            frame.touchpad_down = (buf[base + 32] & 0x80) == 0;
        }
        frame.touchpad_click = (buf[base + 9] & 0x02) != 0;
        if buf.len() > base + 53 {
            let level = (buf[base + 52] & 0x0F).min(10);
            frame.battery_pct = Some(level * 10);
        }
    } else {
        // DualShock 4 USB (0x01) or Bluetooth (0x11)
        let base = if buf[0] == 0x01 && buf.len() >= 32 {
            1usize
        } else if buf[0] == 0x11 && buf.len() >= 35 {
            3usize
        } else {
            return frame;
        };

        let gx = i16::from_le_bytes([buf[base + 12], buf[base + 13]]) as f32;
        let gy = i16::from_le_bytes([buf[base + 14], buf[base + 15]]) as f32;
        let gz = i16::from_le_bytes([buf[base + 16], buf[base + 17]]) as f32;
        let ax = i16::from_le_bytes([buf[base + 18], buf[base + 19]]) as f32;
        let ay = i16::from_le_bytes([buf[base + 20], buf[base + 21]]) as f32;
        let az = i16::from_le_bytes([buf[base + 22], buf[base + 23]]) as f32;

        frame.valid = true;
        frame.pitch_dps = gx / 16.4;
        frame.yaw_dps = -gy / 16.4;
        frame.roll_dps = gz / 16.4;
        frame.accel_x = ax / 8192.0;
        frame.accel_y = ay / 8192.0;
        frame.accel_z = az / 8192.0;
        frame.l2_down = buf[base + 7] > 40;
        frame.r2_down = buf[base + 8] > 40;
        frame.touchpad_click = (buf[base + 6] & 0x02) != 0;
        if buf.len() > base + 34 {
            frame.touchpad_down = (buf[base + 34] & 0x80) == 0;
        }
        let level = (buf[base + 29] & 0x0F).min(10);
        frame.battery_pct = Some(level * 10);
    }

    frame
}

fn parse_switch_pro_report(buf: &[u8]) -> GyroImuFrame {
    let mut frame = GyroImuFrame::default();
    if buf[0] != 0x30 || buf.len() < 25 {
        return frame;
    }
    let ax = i16::from_le_bytes([buf[13], buf[14]]) as f32;
    let ay = i16::from_le_bytes([buf[15], buf[16]]) as f32;
    let az = i16::from_le_bytes([buf[17], buf[18]]) as f32;
    let gx = i16::from_le_bytes([buf[19], buf[20]]) as f32;
    let gy = i16::from_le_bytes([buf[21], buf[22]]) as f32;
    let gz = i16::from_le_bytes([buf[23], buf[24]]) as f32;

    frame.valid = true;
    frame.pitch_dps = gy / 14.28;
    frame.yaw_dps = -gz / 14.28;
    frame.roll_dps = gx / 14.28;
    frame.accel_x = ax / 4096.0;
    frame.accel_y = ay / 4096.0;
    frame.accel_z = az / 4096.0;
    frame.l2_down = (buf[5] & 0x80) != 0;
    frame.r2_down = (buf[3] & 0x80) != 0;
    frame
}

fn enumerate_raw_mice() -> Vec<(String, u16, u16)> {
    let mut mice = Vec::new();
    let mut count: u32 = 0;
    let elem_size = std::mem::size_of::<RAWINPUTDEVICELIST>() as u32;
    unsafe {
        if GetRawInputDeviceList(None, &mut count, elem_size) != 0 || count == 0 {
            return mice;
        }
        let mut list = vec![RAWINPUTDEVICELIST::default(); count as usize];
        let written = GetRawInputDeviceList(Some(list.as_mut_ptr()), &mut count, elem_size);
        if written == u32::MAX {
            return mice;
        }
        for dev in list.into_iter().take(written as usize) {
            if dev.dwType != RIM_TYPEMOUSE {
                continue;
            }
            let hdev: Option<HANDLE> = Some(dev.hDevice);
            let mut info = RID_DEVICE_INFO {
                cbSize: std::mem::size_of::<RID_DEVICE_INFO>() as u32,
                ..Default::default()
            };
            let mut info_sz = info.cbSize;
            let _ = GetRawInputDeviceInfoW(
                hdev,
                RIDI_DEVICEINFO,
                Some((&mut info as *mut RID_DEVICE_INFO).cast()),
                &mut info_sz,
            );

            let mut name_len: u32 = 0;
            let _ = GetRawInputDeviceInfoW(hdev, RIDI_DEVICENAME, None, &mut name_len);
            let mut vid = 0u16;
            let mut pid = 0u16;
            let mut friendly = "USB / Wireless Mouse".to_string();

            if name_len > 0 && name_len < 512 {
                let mut wbuf = vec![0u16; name_len as usize];
                let r = GetRawInputDeviceInfoW(
                    hdev,
                    RIDI_DEVICENAME,
                    Some(wbuf.as_mut_ptr().cast()),
                    &mut name_len,
                );
                if r != u32::MAX {
                    let path = String::from_utf16_lossy(&wbuf);
                    if let Some((v, p)) = parse_vid_pid_from_path(&path) {
                        vid = v;
                        pid = p;
                        friendly = describe_mouse_vid_pid(vid, pid);
                    }
                }
            }
            mice.push((friendly, vid, pid));
        }
    }
    mice
}

fn parse_vid_pid_from_path(path: &str) -> Option<(u16, u16)> {
    let upper = path.to_ascii_uppercase();
    let v_idx = upper.find("VID_")?;
    let p_idx = upper.find("PID_")?;
    let v = u16::from_str_radix(upper.get(v_idx + 4..v_idx + 8)?, 16).ok()?;
    let p = u16::from_str_radix(upper.get(p_idx + 4..p_idx + 8)?, 16).ok()?;
    Some((v, p))
}

fn describe_mouse_vid_pid(vid: u16, pid: u16) -> String {
    match (vid, pid) {
        (0x046D, 0xC53F) | (0x046D, 0x4074) => "Logitech G305 Lightspeed".to_string(),
        (0x046D, 0xC548) | (0x046D, 0xB02A) => "Logitech Signature M650 L".to_string(),
        (0x046D, 0xC08B) | (0x046D, 0xC093) => "Logitech G Pro / G502".to_string(),
        (0x046D, _) => format!("Logitech Mouse ({:04X}:{:04X})", vid, pid),
        _ => format!("RawInput Mouse ({:04X}:{:04X})", vid, pid),
    }
}
