//! Raw HID device enumerator, EdgeTX/OpenTX/ExpressLRS 16-channel normalizer,
//! Betaflight RC Rate/SuperRate/Expo math, and MAVLink v2 RC override frame encoder.

use hidapi::HidApi;
use serde::{Deserialize, Serialize};

pub const MAVLINK_V2_STX: u8 = 0xFD;
pub const MAVLINK_MSG_ID_RC_CHANNELS_OVERRIDE: u32 = 70;
pub const MAVLINK_CRC_EXTRA_RC_CHANNELS_OVERRIDE: u8 = 124;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceKind {
    Xbox,
    LogitechMouse,
    RawMouse,
    FpvTransmitter,
    PlayStation,
    SwitchPro,
    GenericHid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HidDeviceSummary {
    pub id: String,
    pub kind: DeviceKind,
    pub class_name: String,
    pub name: String,
    pub vid: u16,
    pub pid: u16,
    pub usage_page: u16,
    pub usage: u16,
    pub interface_number: i32,
    pub transport: String,
    pub battery_pct: Option<u8>,
    pub battery_mv: Option<u16>,
    pub polling_hz: Option<u32>,
    pub supports_gyro: bool,
    pub supports_hidpp: bool,
    pub supports_fpv: bool,
    pub supports_rumble: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FpvTelemetry {
    pub connected: bool,
    pub name: String,
    pub channels_raw: [f32; 16],
    pub channels_out: [f32; 16],
    pub channel_count: usize,
    pub switches: [u8; 5],
    pub roll_dps: f32,
    pub pitch_dps: f32,
    pub yaw_dps: f32,
}

impl Default for FpvTelemetry {
    fn default() -> Self {
        Self {
            connected: false,
            name: String::new(),
            channels_raw: [0.0; 16],
            channels_out: [0.0; 16],
            channel_count: 0,
            switches: [0; 5],
            roll_dps: 0.0,
            pitch_dps: 0.0,
            yaw_dps: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AxisRateConfig {
    pub rc_rate: f32,
    pub super_rate: f32,
    pub expo: f32,
}

impl Default for AxisRateConfig {
    fn default() -> Self {
        Self {
            rc_rate: 1.0,
            super_rate: 0.70,
            expo: 0.15,
        }
    }
}

pub fn classify_hid_device(vid: u16, pid: u16, up: u16, usage: u16, product: &str) -> DeviceKind {
    let lower = product.to_ascii_lowercase();
    if vid == 0x045E && (up == 0x01 && (usage == 0x04 || usage == 0x05)) {
        return DeviceKind::Xbox;
    }
    if vid == 0x054C && matches!(pid, 0x05C4 | 0x09CC | 0x0BA0 | 0x0CE6 | 0x0DF2) {
        return DeviceKind::PlayStation;
    }
    if vid == 0x057E && pid == 0x2009 {
        return DeviceKind::SwitchPro;
    }
    if vid == 0x046D {
        return DeviceKind::LogitechMouse;
    }
    if (up == 0x01 && (usage == 0x04 || usage == 0x05))
        && (vid == 0x1209
            || vid == 0x0483
            || vid == 0x2DAE
            || vid == 0x2757
            || lower.contains("edgetx")
            || lower.contains("opentx")
            || lower.contains("radiomaster")
            || lower.contains("expresslrs")
            || lower.contains("betafpv")
            || lower.contains("jumper")
            || lower.contains("tbs")
            || lower.contains("tx16")
            || lower.contains("pocket")
            || lower.contains("boxer"))
    {
        return DeviceKind::FpvTransmitter;
    }
    DeviceKind::GenericHid
}

pub fn enumerate_hid_devices() -> Result<Vec<HidDeviceSummary>, String> {
    let api = HidApi::new().map_err(|e| format!("Failed initializing HidApi: {}", e))?;
    let mut out = Vec::new();
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
        let path_str = info.path().to_string_lossy();
        let id = format!("hid-{:04x}-{:04x}-{}-{}", vid, pid, iface, up);
        if out.iter().any(|existing: &HidDeviceSummary| existing.id == id) {
            continue;
        }
        let kind = classify_hid_device(vid, pid, up, usage, &product);
        let transport = if path_str.to_ascii_lowercase().contains("bth")
            || path_str.contains("{00001124")
        {
            "Bluetooth".to_string()
        } else if vid == 0x046D && (pid == 0xC53F || pid == 0xC548 || pid == 0xC52B) {
            "Lightspeed / Receiver".to_string()
        } else {
            "USB".to_string()
        };

        out.push(HidDeviceSummary {
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
            supports_gyro: matches!(kind, DeviceKind::PlayStation | DeviceKind::SwitchPro),
            supports_hidpp: kind == DeviceKind::LogitechMouse && (up == 0xFF00 || up == 0xFF01),
            supports_fpv: kind == DeviceKind::FpvTransmitter,
            supports_rumble: matches!(kind, DeviceKind::Xbox | DeviceKind::PlayStation),
        });
    }
    Ok(out)
}

/// Stateful 16-channel EdgeTX / OpenTX / ExpressLRS USB HID normalizer with dynamic bounds calibration.
#[derive(Debug, Clone)]
pub struct FpvChannelNormalizer {
    pub min_bounds: [i32; 16],
    pub max_bounds: [i32; 16],
}

impl Default for FpvChannelNormalizer {
    fn default() -> Self {
        Self {
            min_bounds: [0; 16],
            max_bounds: [2047; 16],
        }
    }
}

impl FpvChannelNormalizer {
    pub fn reset(&mut self) {
        self.min_bounds = [0; 16];
        self.max_bounds = [2047; 16];
    }

    pub fn normalize_report(&mut self, buf: &[u8], out: &mut FpvTelemetry) {
        parse_fpv_hid_report(buf, &mut self.min_bounds, &mut self.max_bounds, out);
    }
}

/// Parses an EdgeTX / OpenTX / ExpressLRS USB HID joystick report into normalized `[-1.0, 1.0]`
/// values across up to 16 channels and extracts 5 3-position switches (`SA..SE`).
pub fn parse_fpv_hid_report(
    buf: &[u8],
    min_bounds: &mut [i32; 16],
    max_bounds: &mut [i32; 16],
    out: &mut FpvTelemetry,
) {
    let offset = if buf.len() >= 19 && buf[0] == 0x01 {
        4
    } else if buf.len() >= 19 {
        3
    } else {
        0
    };

    let avail = buf.len().saturating_sub(offset);
    let ch_count = (avail / 2).min(16);
    if ch_count == 0 {
        return;
    }

    out.channel_count = ch_count;
    for i in 0..ch_count {
        let idx = offset + i * 2;
        let raw_u16 = u16::from_le_bytes([buf[idx], buf[idx + 1]]);
        let raw_val = if raw_u16 > 4096 {
            (raw_u16 as i16) as i32
        } else {
            raw_u16 as i32
        };

        if raw_val < min_bounds[i] {
            min_bounds[i] = raw_val;
        }
        if raw_val > max_bounds[i] {
            max_bounds[i] = raw_val;
        }
        let span = (max_bounds[i] - min_bounds[i]).max(1) as f32;
        let norm = (((raw_val - min_bounds[i]) as f32 / span) * 2.0 - 1.0).clamp(-1.0, 1.0);
        out.channels_raw[i] = norm;
    }

    for s in 0..5 {
        if 4 + s < ch_count {
            let v = out.channels_raw[4 + s];
            out.switches[s] = if v < -0.35 {
                0
            } else if v > 0.35 {
                2
            } else {
                1
            };
        } else if buf.len() >= 4 {
            let btn_byte = buf[if buf[0] == 0x01 { 1 } else { 0 }];
            out.switches[s] = if (btn_byte & (1 << s)) != 0 { 2 } else { 0 };
        }
    }
}

/// Reorders primary FPV channels from `AETR` or `TAER` into `(roll, pitch, throttle, yaw)`.
#[inline]
pub fn reorder_fpv_primary_axes(raw: &[f32; 16], order: &str) -> (f32, f32, f32, f32) {
    if order.eq_ignore_ascii_case("TAER") {
        (raw[1], raw[2], raw[0], raw[3])
    } else {
        (raw[0], raw[1], raw[2], raw[3])
    }
}

/// Betaflight legacy/classic RC Rate + SuperRate + Expo formula.
/// Returns `(normalized_output [-1..1], angular_velocity_deg_per_sec)`.
#[inline]
pub fn apply_betaflight_rate(rc_input: f32, cfg: &AxisRateConfig) -> (f32, f32) {
    let clamped = rc_input.clamp(-1.0, 1.0);
    let abs_in = clamped.abs();
    let rc_rate = cfg.rc_rate.clamp(0.1, 2.5);
    let super_rate = cfg.super_rate.clamp(0.0, 0.95);
    let expo = cfg.expo.clamp(0.0, 1.0);

    let expo_factor = clamped * (abs_in * abs_in * abs_in * expo + (1.0 - expo));
    let super_denom = (1.0 - abs_in * super_rate).max(0.02);
    let deg_per_sec = 200.0 * rc_rate * expo_factor / super_denom;
    let max_dps = max_betaflight_dps(cfg);
    ((deg_per_sec / max_dps).clamp(-1.0, 1.0), deg_per_sec)
}

#[inline]
pub fn max_betaflight_dps(cfg: &AxisRateConfig) -> f32 {
    let rc_rate = cfg.rc_rate.clamp(0.1, 2.5);
    let super_rate = cfg.super_rate.clamp(0.0, 0.95);
    (200.0 * rc_rate / (1.0 - super_rate).max(0.02)).max(1.0)
}

pub fn sample_betaflight_preview(cfg: &AxisRateConfig, steps: usize) -> Vec<[f32; 2]> {
    let n = steps.clamp(16, 128);
    let mut out = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let x = i as f32 / n as f32;
        let (norm, _) = apply_betaflight_rate(x, cfg);
        out.push([x, norm.abs()]);
    }
    out
}

/// Converts a normalized RC axis (`[-1.0, 1.0]` or `[0.0, 1.0]` when `unipolar` is true)
/// into standard `1000..=2000` us PWM.
#[inline]
pub fn rc_normalized_to_pwm(norm: f32, unipolar: bool) -> u16 {
    let unit = if unipolar {
        norm.clamp(0.0, 1.0)
    } else {
        (norm.clamp(-1.0, 1.0) + 1.0) * 0.5
    };
    (1000.0 + unit * 1000.0).round().clamp(1000.0, 2000.0) as u16
}

/// Computes MAVLink X.25 CRC-16/MCRF4XX checksum with `crc_extra`.
#[inline]
pub fn mavlink_x25_crc(bytes: &[u8], crc_extra: u8) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in bytes.iter().chain(std::iter::once(&crc_extra)) {
        let mut tmp = b ^ (crc as u8);
        tmp ^= tmp << 4;
        crc = (crc >> 8) ^ ((tmp as u16) << 8) ^ ((tmp as u16) << 3) ^ ((tmp as u16) >> 4);
    }
    crc
}

/// Encodes a zero-allocation MAVLink v2 `RC_CHANNELS_OVERRIDE` (message ID 70) packet
/// into a 50-byte stack buffer, returning `(frame_bytes, frame_len)`.
///
/// Channels 17 & 18 may be set to `0` (or `u16::MAX` to ignore); trailing zero bytes
/// are trimmed in accordance with the MAVLink v2 framing specification.
pub fn encode_mavlink_v2_rc_override_pwm(
    channels_pwm: &[u16; 18],
    target_system: u8,
    target_component: u8,
    sysid: u8,
    compid: u8,
    seq: u8,
) -> ([u8; 50], usize) {
    let mut payload = [0u8; 38];
    for (i, &ch) in channels_pwm[..8].iter().enumerate() {
        let le = ch.to_le_bytes();
        payload[i * 2] = le[0];
        payload[i * 2 + 1] = le[1];
    }
    payload[16] = target_system;
    payload[17] = target_component;
    for (i, &ch) in channels_pwm[8..18].iter().enumerate() {
        let le = ch.to_le_bytes();
        payload[18 + i * 2] = le[0];
        payload[18 + i * 2 + 1] = le[1];
    }

    // MAVLink v2 trailing-zero payload truncation (minimum 1 byte)
    let mut payload_len = payload.len();
    while payload_len > 1 && payload[payload_len - 1] == 0 {
        payload_len -= 1;
    }

    let mut frame = [0u8; 50];
    frame[0] = MAVLINK_V2_STX;
    frame[1] = payload_len as u8;
    frame[2] = 0; // incompat_flags
    frame[3] = 0; // compat_flags
    frame[4] = seq;
    frame[5] = sysid;
    frame[6] = compid;
    let msgid_bytes = MAVLINK_MSG_ID_RC_CHANNELS_OVERRIDE.to_le_bytes();
    frame[7] = msgid_bytes[0];
    frame[8] = msgid_bytes[1];
    frame[9] = msgid_bytes[2];
    frame[10..10 + payload_len].copy_from_slice(&payload[..payload_len]);

    let crc = mavlink_x25_crc(
        &frame[1..10 + payload_len],
        MAVLINK_CRC_EXTRA_RC_CHANNELS_OVERRIDE,
    );
    let crc_le = crc.to_le_bytes();
    frame[10 + payload_len] = crc_le[0];
    frame[11 + payload_len] = crc_le[1];

    (frame, 12 + payload_len)
}

/// Encodes 16 normalized RC channels (`[-1.0, 1.0]`, with optional unipolar `[0.0, 1.0]`
/// throttle index) as a MAVLink v2 `RC_CHANNELS_OVERRIDE` (#70) frame.
pub fn encode_mavlink_v2_rc_override_normalized(
    channels_norm: &[f32; 16],
    unipolar_channel_idx: Option<usize>,
    target_system: u8,
    target_component: u8,
    sysid: u8,
    compid: u8,
    seq: u8,
) -> ([u8; 50], usize) {
    let mut pwm = [0u16; 18];
    for (i, &val) in channels_norm.iter().enumerate() {
        let unipolar = unipolar_channel_idx == Some(i);
        pwm[i] = rc_normalized_to_pwm(val, unipolar);
    }
    encode_mavlink_v2_rc_override_pwm(&pwm, target_system, target_component, sysid, compid, seq)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn betaflight_rate_endpoints_and_expo() {
        let rates = AxisRateConfig {
            rc_rate: 1.0,
            super_rate: 0.70,
            expo: 0.20,
        };
        let (norm, dps) = apply_betaflight_rate(1.0, &rates);
        assert!((norm - 1.0).abs() < 1e-4);
        assert!((dps - max_betaflight_dps(&rates)).abs() < 1e-2);
        let (zero_norm, zero_dps) = apply_betaflight_rate(0.0, &rates);
        assert_eq!(zero_norm, 0.0);
        assert_eq!(zero_dps, 0.0);
    }

    #[test]
    fn mavlink_v2_rc_override_framing_and_crc() {
        let mut norm = [0.0f32; 16];
        norm[0] = -1.0; // 1000
        norm[1] = 1.0; // 2000
        norm[2] = 0.5; // 1500 unipolar
        let (frame, len) = encode_mavlink_v2_rc_override_normalized(
            &norm,
            Some(2),
            1,
            1,
            255,
            190,
            7,
        );
        assert_eq!(frame[0], 0xFD);
        assert_eq!(frame[4], 7);
        assert_eq!(frame[5], 255);
        assert_eq!(frame[6], 190);
        assert_eq!(frame[7], 70);
        assert_eq!(frame[8], 0);
        assert_eq!(frame[9], 0);
        // Ch1 = 1000, Ch2 = 2000, Ch3 = 1500
        assert_eq!(u16::from_le_bytes([frame[10], frame[11]]), 1000);
        assert_eq!(u16::from_le_bytes([frame[12], frame[13]]), 2000);
        assert_eq!(u16::from_le_bytes([frame[14], frame[15]]), 1500);
        // Last two bytes must match MAVLink X.25 CRC
        let payload_len = frame[1] as usize;
        assert_eq!(len, 12 + payload_len);
        let expected_crc = mavlink_x25_crc(&frame[1..10 + payload_len], 124);
        let wire_crc = u16::from_le_bytes([frame[len - 2], frame[len - 1]]);
        assert_eq!(wire_crc, expected_crc);
    }

    #[test]
    fn edgetx_16ch_normalizer_parses_axes_and_switches() {
        let mut raw = [0u8; 36]; // 4 byte header + 16 * 2 byte channels
        raw[0] = 0x01;
        // Ch0 = 0, Ch1 = 1024, Ch2 = 2047, Ch4 (SA) = 2047 (high)
        let ch1 = 1024u16.to_le_bytes();
        raw[6] = ch1[0];
        raw[7] = ch1[1];
        let ch2 = 2047u16.to_le_bytes();
        raw[8] = ch2[0];
        raw[9] = ch2[1];
        let ch4 = 2047u16.to_le_bytes();
        raw[12] = ch4[0];
        raw[13] = ch4[1];

        let mut norm = FpvChannelNormalizer::default();
        let mut tel = FpvTelemetry::default();
        norm.normalize_report(&raw, &mut tel);
        assert_eq!(tel.channel_count, 16);
        assert!((tel.channels_raw[0] - (-1.0)).abs() < 1e-3);
        assert!(tel.channels_raw[1].abs() < 0.01);
        assert!((tel.channels_raw[2] - 1.0).abs() < 1e-3);
        assert_eq!(tel.switches[0], 2);
    }
}
